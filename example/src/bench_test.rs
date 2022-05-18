use fast_log::appender::{LogAppender, FastLogRecord};
use std::time::{Instant};
use fast_log::bencher::QPS;
use fast_log::config::Config;

/// cargo run --release --package example --bin bench_test
fn main() {
    struct BenchRecvLog {}
    impl LogAppender for BenchRecvLog {
        fn do_log(&self, _: &FastLogRecord) {
            //do nothing
        }
    }
    fast_log::init(Config::new().custom(BenchRecvLog{}));
    let total = 1000000;
    let now = Instant::now();
    for index in 0..total {
        log::info!("Commencing yak shaving{}", index);
    }
    //wait log finish write all
    log::logger().flush();
    now.time(total);
    now.qps(total);
}