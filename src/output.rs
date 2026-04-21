use colored::Colorize;

pub fn log(msg: &str) {
    println!("{} {}", "[wt]".on_green().white(), msg);
}
