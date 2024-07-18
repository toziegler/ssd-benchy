#!/bin/bash

for util in 0.5; do
    echo "space util $util"
    #numactl -C 0-47 ./target/release/ssd-benchy --ssd-device nvme1n1 --max-iops 200000 --utilization-iops 1.0 1.2 1.4 1.6 1.8 --serialize-samples --runtime-seconds=300 --instance-type i3en.12xlarge --use-fsync --writer-threads 42 --preinitialize --capacity-fraction $util
    numactl -C 0-47 ./target/release/ssd-benchy --ssd-device nvme1n1 --max-iops 200000 --utilization-iops 1.0 1.2 1.4 1.6 1.8 --serialize-samples --runtime-seconds=300 --instance-type i3en.12xlarge --use-fsync --writer-threads 42 --capacity-fraction $util
    numactl -C 0-47 ./target/release/ssd-benchy --ssd-device nvme1n1 --max-iops 200000 --utilization-iops 1.0 1.2 1.4 1.6 1.8 --serialize-samples --runtime-seconds=300 --instance-type i3en.12xlarge --use-fsync --writer-threads 42 --capacity-fraction $util --spiky
done
