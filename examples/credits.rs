// EMAKI_API_KEY=sk-... cargo run --example credits
fn main() {
    match emaki::Client::from_env().and_then(|c| c.credits()) {
        Ok(n) => println!("{n}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
