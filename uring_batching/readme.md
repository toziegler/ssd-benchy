# Batching test 

The finding was that if multiple threads write at the same time (spiky) the latency is bad. 
The question now arises if batching in io uring has similar negative effects on latency. 

I.e. Batching sucks? 
