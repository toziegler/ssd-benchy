library(ggplot2)
library(sqldf)
library(tidyr)
library(dplyr)
options(scipen = 999)


# Summary
summary <- read.csv("./summary_file.csv")
# Reshape data
long_data <- summary %>%
    select(instance_type, start_time, p50th, p75th, utilization_iop) %>%
    pivot_longer(cols = c(p50th, p75th), names_to = "percentile", values_to = "value")

# Plot using ggplot2
ggplot(long_data, aes(x = utilization_iop, y = value, group = utilization_iop)) +
    # geom_line() +
    geom_bar(stat = "identity") +
    facet_wrap(~percentile, scales = "free_x") +
    labs(
        title = "50th and 75th Percentiles Over Time",
        x = "Start Time",
        y = "Latency (ms)",
        color = "Percentile"
    ) +
    theme_bw()




# Samples
samples <- read.csv("./samples_file.csv")


samples <- sqldf("SELECT * FROM samples where id > 1000")

ggplot(samples, aes(x = id, y = latency / 1e4)) +
    geom_line() +
    geom_point() +
    theme_bw()
