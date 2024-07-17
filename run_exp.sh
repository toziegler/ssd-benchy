#!/bin/bash

#for util in 0.1 0.2 0.3 0.4 0.5 0.6 0.7 0.8; do
for util in 0.5; do
    echo "space util $util"
    numactl -C 0-47 ./target/release/ssd-benchy --ssd-device nvme1n1 --max-iops 200000 --utilization-iops 0.5 0.6 0.7 0.8 --serialize-samples --runtime-seconds=300 --instance-type i3en.12xlarge --use-fsync --writer-threads 42 --preinitialize --capacity-fraction $util
    numactl -C 0-47 ./target/release/ssd-benchy --ssd-device nvme1n1 --max-iops 200000 --utilization-iops 0.5 0.6 0.7 0.8 --serialize-samples --runtime-seconds=300 --instance-type i3en.12xlarge --use-fsync --writer-threads 42 --capacity-fraction $util --spiky
done
