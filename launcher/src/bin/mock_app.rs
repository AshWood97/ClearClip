use std::env;
use std::fs;

fn main() {
    let current_exe = env::current_exe().unwrap();
    let current_dir = env::current_dir().unwrap();
    let args: Vec<String> = env::args().collect();

    let log_content = format!(
        "Executable: {}\nWorking Dir: {}\nArgs: {:?}\n",
        current_exe.to_string_lossy(),
        current_dir.to_string_lossy(),
        args
    );

    let log_path = current_exe.parent().unwrap().join("mock_run.log");
    fs::write(&log_path, log_content).unwrap();
}
