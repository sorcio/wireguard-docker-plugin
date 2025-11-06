use std::process::{Child, Command};

#[allow(dead_code)]
pub trait CommandExt {
    /// Run the command and expect it to succeed, returning the output.
    fn expect_output(&mut self) -> std::process::Output;

    /// Get the command line arguments represented as a printable string.
    fn get_command_line(&self) -> String;

    /// Run the command and expect it to succeed, ignoring the output.
    fn expect_success(&mut self);

    /// Spawn the command and return a guard that will interrupt it when dropped.
    fn spawn_with_guard(&mut self) -> std::io::Result<InterruptOnDrop>;
}

impl CommandExt for Command {
    fn get_command_line(&self) -> String {
        let program = self.get_program().to_string_lossy().to_string();
        let args = self
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        format!("{} {}", program, args.join(" "))
    }

    fn expect_output(&mut self) -> std::process::Output {
        match self.output() {
            Err(err) => {
                let cmdline = self.get_command_line();
                panic!("Error running command {cmdline:?}: {err}");
            }
            Ok(output) => {
                if !output.status.success() {
                    let cmdline = self.get_command_line();
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    panic!("Command failed {cmdline:?}:\n--------------\n{stderr}\n--------------");
                }
                output
            }
        }
    }

    fn expect_success(&mut self) {
        self.expect_output();
    }

    fn spawn_with_guard(&mut self) -> std::io::Result<InterruptOnDrop> {
        let child = self.spawn()?;
        Ok(InterruptOnDrop::new(child))
    }
}

#[derive(Debug)]
pub struct InterruptOnDrop(Child);

impl Drop for InterruptOnDrop {
    fn drop(&mut self) {
        use rustix::process::Pid;
        let _ =
            rustix::process::kill_process(Pid::from_child(&self.0), rustix::process::Signal::INT);
        if let Err(err) = self.0.wait() {
            eprintln!("Error waiting for child process: {err}");
        }
    }
}

impl InterruptOnDrop {
    pub fn new(child: Child) -> Self {
        InterruptOnDrop(child)
    }
}

impl core::ops::Deref for InterruptOnDrop {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for InterruptOnDrop {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
