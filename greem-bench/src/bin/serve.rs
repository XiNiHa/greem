//! `PORT=4001 cargo run -p greem-bench --release --bin serve`: the greem
//! implementation for the driver.

#[tokio::main]
async fn main() {
    greem_bench::serve::serve().await;
}
