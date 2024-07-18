const std = @import("std");

pub const C = @import("liburing");

pub fn main() !void {
    // ------------- Constants
    const initialized_blocks = 800000000;
    const duration = 3; // seconds
    const rate = 100000;
    const io_batch_size = 8;
    const io_rate = rate / io_batch_size;
    const io_interval = std.time.ns_per_s / io_rate;

    std.debug.print("io_interval {} \n", .{io_interval});
    // ------------- SSD
    const ssd_path = "/dev/nvme1n1";
    const flags: std.posix.O = .{ .DIRECT = true, .ACCMODE = .RDWR };
    const ssd_fd = std.posix.open(ssd_path, flags, 0o666) catch |err| switch (err) {
        error.FileNotFound => std.debug.panic("No block device available at {s}", .{ssd_path}),
        else => return err,
    };
    defer std.posix.close(ssd_fd);

    // ------------ IO URING SETUP
    var ring = std.mem.zeroes(C.io_uring);
    var params = std.mem.zeroes(C.io_uring_params);
    params.flags |= C.IORING_SETUP_SINGLE_ISSUER | C.IORING_SETUP_DEFER_TASKRUN;
    if (C.io_uring_queue_init_params(128, &ring, &params) < 0) {
        @panic("Failed to init ring");
    }

    // -------------- Experiment
    var timer_experiment = try std.time.Timer.start();
    var timer_io_schedule = try std.time.Timer.start();

    // schedule at which point we send X requests, user_data time stamp
    // poll completion until the requests are all completed
    // check if we are behind schdule and adjust latency

    const io_buffer: [4096]u8 align(4096) = [_]u8{7} ** 4096;
    var samples: [4096]i64 = [_]i64{0} ** 4096;
    var io_offset: u64 = 0;
    var sample_i: usize = 0;

    while (timer_experiment.read() / std.time.ns_per_s < duration) {
        if (timer_io_schedule.read() < io_interval) {
            continue;
        }
        // if we lack behind the schedule we calculate the diff
        const diff = (timer_io_schedule.read() - io_interval) / std.time.ns_per_us;
        timer_io_schedule.reset(); // reset the timer
        for (0..io_batch_size) |_| {
            if (io_offset >= initialized_blocks) {
                io_offset = 0;
            }
            const sqe = C.io_uring_get_sqe(&ring);
            C.io_uring_prep_write(sqe, ssd_fd, &io_buffer, 4096, io_offset * 4096);
            const time_stamp: u64 = @intCast(std.time.microTimestamp());
            C.io_uring_sqe_set_data64(
                sqe,
                time_stamp,
            );
            io_offset += 1;
        }

        var completions: u16 = 0;
        while (completions < io_batch_size) {
            _ = C.io_uring_submit_and_wait(&ring, 1);
            var it = C.CQEIterator.init(&ring);
            while (it.next()) |cqe| {
                if (cqe.res < 0) {
                    @panic("cqe failed");
                }

                const begin_time: i64 = @intCast(C.io_uring_cqe_get_data64(cqe));
                const end_time = std.time.microTimestamp();

                const latency = end_time - begin_time + @as(i64, @intCast(diff));
                //_ = latency;
                //std.debug.print("latency {} \n", .{latency});
                if (sample_i < 4096) {
                    samples[sample_i] = latency;
                    sample_i += 1;
                }

                completions += 1;
            }
            it.advance();
        }
    }
    std.debug.print("{any}", .{samples});
}
