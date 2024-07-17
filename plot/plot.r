library(ggplot2)
library(sqldf)
library(tidyr)
library(dplyr)
options(scipen = 999)


# Summary
summary <- read.csv("./summary_file.csv")
# Reshape data
long_data <- summary %>%
    select(instance_type, start_time, p50th, p75th, p90th, p99th, p999th, utilization_iop, spiky, capacity_fraction) %>%
    pivot_longer(cols = c(p50th, p75th, , p90th, p99th , p999th), names_to = "percentile", values_to = "value")

# Plot using ggplot2
ggplot(long_data, aes(x = utilization_iop, y = value / 1000, color = percentile)) +
    geom_line() +
    geom_point() +
    facet_grid(spiky ~ capacity_fraction) + 
    coord_cartesian(ylim = c(0, 500)) +
    labs(
        x = "utilization",
        y = "latency (microseconds)",
        color = "Percentile"
    ) +
    theme_bw()


# Specify any integer
set.seed(1) 
# Samples
samples <- read.csv("./samples_file.csv")
#samples <- sqldf("SELECT * FROM samples where id > 1000")
sampled_data <- sample_n(samples, 10000)


joined <- sqldf("SELECT utilization_iop, spiky, capacity_fraction, latency, id  FROM summary as s, sampled_data as d WHERE s.uuid = d.uuid")

spiky  <- sqldf("SELECT * FROM joined WHERE spiky like 'true'")

ggplot(spiky, aes(x = id, y = latency / 1000)) +
    facet_grid(utilization_iop ~ capacity_fraction, scales = "free") + 
    coord_cartesian(ylim = c(0, 500)) +
    geom_line() +
    theme_bw()

not_spiky  <- sqldf("SELECT * FROM joined WHERE spiky like 'false'")

ggplot(not_spiky, aes(x = id, y = latency / 1000)) +
    facet_grid(utilization_iop ~ capacity_fraction, scales = "free") + 
    coord_cartesian(ylim = c(0, 550)) +
    geom_line() +
    theme_bw()

# TODO: 
#
# Experiment 
# Run over night single writer with little capacity and utilization to see if spikes are introduced by noisy neighbours 
# noisy neighbour? 
