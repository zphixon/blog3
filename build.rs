use std::process::Command;

fn main() -> Result<(), ()> {
    println!("cargo:rerun-if-changed=generate.sql");

    let output = Command::new("sqlite3")
        .arg("-bail")
        .arg("example.sqlite3")
        .arg(".read generate.sql")
        .arg(".exit")
        .output()
        .expect("sqlite3");

    if !output.status.success() {
        eprintln!(
            "stdout: {}",
            std::str::from_utf8(&output.stdout).expect("stdout")
        );
        eprintln!(
            "stderr: {}",
            std::str::from_utf8(&output.stderr).expect("stderr")
        );
        return Err(());
    }

    Ok(())
}
