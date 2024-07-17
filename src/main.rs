/*!
A SSD latency benchmarking tool.

This tool is designed to measure and analyze the latency of Solid State Drives (SSDs) under various conditions.
It allows you to configure multiple parameters to simulate real-world workloads and gather detailed performance metrics.

## Write Pattern
Each thread writes to its designated region sequentially until it wraps around. The size of these regions is determined based on the `preinitialized_fraction`.

## Usage
To use this tool, you can specify the parameters via command-line arguments. Here is an example:

```sh
ssd-benchy --ssd-device nvme1n1 --max-iops 200000 --utilization-iops 0.5 0.6 0.7 --serialize-samples  --runtime-seconds=300 --instance-type i3en.3xlarge --use-fsync
```
*/

use gethostname::gethostname;
use libc::{O_DIRECT, O_RDWR};
use serde::Serialize;
use std::{
    arch::x86_64::_mm_pause,
    fs::{self, OpenOptions},
    ops::Range,
    os::unix::fs::{FileExt, OpenOptionsExt},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

use clap::Parser;
#[derive(Parser, Debug, Clone, Serialize)]
#[clap(author, version, about, long_about = None)]
struct CliConfig {
    /// instance type
    #[clap(long, required = true)]
    instance_type: String,

    /// The number of writer threads; must be large enough
    #[clap(long, default_value_t = 10)]
    writer_threads: u64,

    /// preinitialize the capacity first
    #[clap(long, default_value_t = false)]
    preinitialize: bool,

    /// Fraction of the SSD that is being used., 0.8 means 80% (important for benchmarks)
    #[clap(long, default_value_t = 0.8)]
    capacity_fraction: f64,

    /// The maximum specified IOPS of this device (based on the spec)
    #[clap(long)]
    max_iops: u64,

    /// The utilization levels at which the benchmark is performed, e.g., 0.6 0.7
    #[clap(long, value_parser, num_args = 1.., value_delimiter = ' ', required = true)]
    utilization_iops: Vec<f64>,

    /// Use fsync after every write
    #[clap(long, default_value_t = false)]
    use_fsync: bool,

    /// serialize the full sample vector
    #[clap(long, default_value_t = false)]
    serialize_samples: bool,

    /// Forces the threads to write roughly at the same time creating a micro spike but still
    /// maintains the rate
    #[clap(long, default_value_t = false)]
    spiky: bool,

    /// Name of the SSD device, e.g., /dev/md0; must be the real name of the block device and not an alias
    #[clap(long)]
    ssd_device: String,

    /// The runtime in seconds for each utilization point
    #[clap(long, default_value_t = 10)]
    runtime_seconds: u64,

    /// Result file
    #[clap(long, default_value_t = String::from("summary_file.csv"))]
    summary_file: String,

    /// Name of the SSD device, e.g., /dev/md0; must be the real name of the block device and not an alias
    /// Result file
    #[clap(long, default_value_t = String::from("samples_file.csv"))]
    samples_file: String,
}

/// Describes the current benchmark parameter and environment
#[derive(Serialize, Debug, Clone)]
struct BenchmarkConfig {
    instance_type: String,
    start_time: u64, // start time from unix epoch
    hostname: String,
    ssd_device: String,
    writer_threads: u64,
    runtime_seconds: u64,
    preinitialize: bool,
    capacity_fraction: f64,
    max_iops: u64,
    iops: u64,
    utilization_iop: f64, // single measurement point
    use_fsync: bool,
    uuid: u128,
    spiky: bool,
}

impl BenchmarkConfig {
    pub fn from_cli_config(
        config: &CliConfig,
        iops_utilization: f64,
        uuid: u128,
    ) -> BenchmarkConfig {
        let start_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("");

        BenchmarkConfig {
            instance_type: config.instance_type.clone(),
            start_time: start_time.as_secs(),
            hostname: gethostname().into_string().unwrap(),
            ssd_device: config.ssd_device.clone(),
            writer_threads: config.writer_threads,
            runtime_seconds: config.runtime_seconds,
            preinitialize: config.preinitialize,
            capacity_fraction: config.capacity_fraction,
            max_iops: config.max_iops,
            utilization_iop: iops_utilization,
            iops: (iops_utilization * config.max_iops as f64) as u64,
            use_fsync: config.use_fsync,
            uuid,
            spiky: config.spiky,
        }
    }
}

#[derive(Serialize, PartialEq, PartialOrd, Ord, Eq, Debug)]
struct Sample {
    latency: u128,
    id: u64,
    uuid: u128,
}

#[derive(Serialize, Debug)]
struct SummaryStatistics {
    min: u128,
    max: u128,
    p50th: u128,
    p75th: u128,
    p90th: u128,
    p99th: u128,
    p999th: u128,
}

impl SummaryStatistics {
    fn percentile(samples: &[Sample], percentile: f64) -> &Sample {
        let len = samples.len();
        let index = ((len as f64) * percentile / 100.0).ceil() as usize - 1;
        &samples[index]
    }

    pub fn create_from_sample(samples: &mut [Sample]) -> SummaryStatistics {
        samples.sort_by_key(|sample| sample.latency);
        let min = samples.first().expect("no samples collected").latency;
        let max = samples.last().expect("no samples collected").latency;

        SummaryStatistics {
            min,
            max,
            p50th: SummaryStatistics::percentile(&samples, 50.0).latency,
            p75th: SummaryStatistics::percentile(&samples, 75.0).latency,
            p90th: SummaryStatistics::percentile(&samples, 90.0).latency,
            p99th: SummaryStatistics::percentile(&samples, 99.0).latency,
            p999th: SummaryStatistics::percentile(&samples, 99.9).latency,
        }
    }
}

#[repr(align(4096))]
struct DirectIOBuffer<const SIZE: usize>([u8; SIZE]);

struct RateLimiter {
    inter_arrival_time: f64,
    next_time: Instant,
}

impl RateLimiter {
    pub fn new(rate: f64, threads: u64, thread_id: u64, spiky: bool) -> Self {
        let rate_per_thread = rate / threads as f64;
        let inter_arrival_time = 1e6 / rate_per_thread; // microseconds
        let mut inter_arrival_time_offset =
            (inter_arrival_time / threads as f64) * thread_id as f64;
        if spiky {
            inter_arrival_time_offset = 0.0; // forces threads to start at roughly the same time
        }
        let next_time = Instant::now()
            + Duration::from_micros(inter_arrival_time_offset as u64 + inter_arrival_time as u64);

        RateLimiter {
            inter_arrival_time,
            next_time,
        }
    }
    // write reate limiter
    fn wait_until(next: Instant) {
        let mut current = Instant::now();
        let mut time_span = next.duration_since(current);

        while time_span.as_secs_f64() > 0.0 {
            current = Instant::now();
            unsafe { _mm_pause() };
            time_span = next.duration_since(current);
        }
    }

    pub fn run<F: FnMut()>(&mut self, mut action: F, mut sampling: impl FnMut(u128)) {
        self.next_time = self.next_time + Duration::from_micros(self.inter_arrival_time as u64);
        let diff = (Instant::now() - self.next_time).as_nanos();
        RateLimiter::wait_until(self.next_time);
        let begin = Instant::now();
        action();
        let mut end = begin.elapsed().as_nanos();
        if diff > 0 {
            end += diff;
        }
        sampling(end);
    }
}

fn get_device_capacity(device_name: &str) -> Result<u64, String> {
    let sys_block_path = format!("/sys/class/block/{}/size", device_name);
    let size_str = fs::read_to_string(Path::new(&sys_block_path))
        .map_err(|_| format!("Failed to read from {}", sys_block_path))?;

    let size_in_sectors: u64 = size_str
        .trim()
        .parse()
        .map_err(|_| format!("Failed to parse size from {}", sys_block_path))?;

    // The size is given in 512-byte sectors, convert to bytes
    let size_in_bytes = size_in_sectors * 512;

    Ok(size_in_bytes)
}

// returns the number of bytes that were intitizlied
fn initialize_ssd(ssd_device: &str, utilization: f64) -> u64 {
    // write sequentially
    const BLOCK_SIZE: usize = 2097152;
    let ssd_capacity_bytes = get_device_capacity(ssd_device).unwrap();
    let scratch_buffer = Box::new(DirectIOBuffer([5; BLOCK_SIZE]));
    let number_ios = ((ssd_capacity_bytes as f64 / BLOCK_SIZE as f64) * utilization) as u64;
    let flags = O_RDWR | O_DIRECT;
    let ssd_path = format!("/dev/{}", ssd_device);
    let ssd_fd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(flags)
        .open(ssd_path)
        .unwrap();

    let mut initialized_bytes = 0;
    for i in 0..number_ios {
        let res = ssd_fd
            .write_at(&scratch_buffer.0, i * BLOCK_SIZE as u64)
            .expect("Could not write");
        assert_eq!(res, BLOCK_SIZE);
        initialized_bytes += res as u64;
    }
    ssd_fd.sync_data().unwrap();
    initialized_bytes
}

fn partition(id: u64, participants: u64, n: u64) -> Range<u64> {
    let block_size = n / participants;
    let begin = id * block_size;
    let mut end = begin + block_size;
    if id == participants - 1 {
        end = n;
    }
    begin..end
}

const BLOCK_SIZE: usize = 4096;

fn main() {
    let config: &'static CliConfig = Box::leak(Box::new(CliConfig::parse()));

    if config.preinitialize {
        println!("Initializing SSDs ... ");
        initialize_ssd(&config.ssd_device, config.capacity_fraction);
        println!(" [Done]");
    } else {
        println!("No preinitialize");
    }

    let initialized_blocks = (get_device_capacity(&config.ssd_device).unwrap() as f64
        * config.capacity_fraction) as u64
        / BLOCK_SIZE as u64;

    for utilization in config.utilization_iops.iter() {
        let uuid = Uuid::new_v4();
        // TODO: atomic counter
        let barrier_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let threads: Vec<_> = (0..config.writer_threads)
            .map(|worker_id| {
                let barrier_counter = barrier_counter.clone();
                std::thread::spawn(move || {
                    let flags = O_RDWR | O_DIRECT;
                    let ssd_path = format!("/dev/{}", config.ssd_device);
                    let ssd_fd = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .custom_flags(flags)
                        .open(ssd_path)
                        .unwrap();
                    let buffer = Box::new(DirectIOBuffer([7; BLOCK_SIZE]));
                    let mut samples = Vec::with_capacity(10000);
                    let write_rate = config.max_iops as f64 * utilization;
                    let range = partition(worker_id, config.writer_threads, initialized_blocks);
                    let mut block_current = range.start;
                    let mut operations = 0;

                    barrier_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                    while barrier_counter.load(std::sync::atomic::Ordering::SeqCst)
                        != config.writer_threads
                    {
                        // spin
                        std::hint::spin_loop();
                    }

                    let mut ratelimiter = RateLimiter::new(
                        write_rate,
                        config.writer_threads,
                        worker_id,
                        config.spiky,
                    );
                    let end_time = Instant::now() + Duration::from_secs(config.runtime_seconds);

                    while Instant::now() < end_time {
                        if block_current >= range.end {
                            block_current = range.start;
                        }
                        ratelimiter.run(
                            || {
                                let res = ssd_fd
                                    .write_at(&buffer.0, block_current * BLOCK_SIZE as u64)
                                    .expect("could not write");
                                if config.use_fsync {
                                    ssd_fd.sync_data().unwrap();
                                }
                                assert_eq!(res, BLOCK_SIZE)
                            },
                            |latency| {
                                if fastrand::u64(0..1000) <= 1 {
                                    samples.push(Sample {
                                        latency,
                                        id: operations,
                                        uuid: uuid.as_u128(),
                                    })
                                }
                            },
                        );
                        operations += 1;
                        block_current += 1;
                    }
                    samples
                })
            })
            .collect();

        let benchmark_config =
            BenchmarkConfig::from_cli_config(config, *utilization, uuid.as_u128());
        let mut samples: Vec<Sample> = vec![];
        for th in threads {
            let mut s = th.join().unwrap();
            samples.append(&mut s);
        }

        let statistic = SummaryStatistics::create_from_sample(&mut samples);

        println!("serializing summary_file");
        //--------- Summary File
        {
            let file_exists = Path::new(&config.summary_file).exists();
            let file = OpenOptions::new()
                .write(true)
                .append(true)
                .create(true)
                .open(&config.summary_file)
                .unwrap();

            let mut wtr = csv::WriterBuilder::new()
                .has_headers(!file_exists)
                .from_writer(file);

            wtr.serialize((benchmark_config.clone(), statistic))
                .unwrap();
            wtr.flush().unwrap();
        }

        println!("serializing samples_file");
        //------ Sample File
        if config.serialize_samples {
            let file_exists = Path::new(&config.samples_file).exists();
            let file = OpenOptions::new()
                .write(true)
                .append(true)
                .create(true)
                .open(&config.samples_file)
                .unwrap();
            let mut wtr = csv::WriterBuilder::new()
                .has_headers(!file_exists)
                .from_writer(file);
            for s in samples {
                wtr.serialize(&s).unwrap();
            }
            wtr.flush().unwrap();
        }
    }
}
