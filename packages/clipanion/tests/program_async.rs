use std::{future::Future, pin::pin, process::ExitCode, rc::Rc, task::{Context, Poll, Waker}};

use clipanion::{advanced::CliAsync, details::{CliEnums, CommandExecutorAsync, CommandProvider}, prelude::*, CommandError, Environment};

const BUFFER_SIZE: usize = 64 * 1024;

/**
 * Returns `Pending` once, so the futures awaiting it really are suspended
 * (and have to keep what they hold across it).
 */
struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future
        = pin!(future);

    let mut cx
        = Context::from_waker(Waker::noop());

    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

fn assert_send<T: Send>(_: &T) {}

#[cli::command]
#[cli::path("small")]
struct SmallCommand {}

impl SmallCommand {
    async fn execute(&self) {
        YieldNow(false).await;
    }
}

#[cli::command]
#[cli::path("large")]
struct LargeCommand {}

impl LargeCommand {
    async fn execute(&self) -> ExitCode {
        let buffer
            = std::hint::black_box([42u8; BUFFER_SIZE]);

        YieldNow(false).await;

        ExitCode::from(buffer[BUFFER_SIZE - 1])
    }
}

#[cli::command]
#[cli::path("larger")]
struct LargerCommand {}

impl LargerCommand {
    async fn execute(&self) -> CommandError {
        let buffer
            = std::hint::black_box([0u8; 2 * BUFFER_SIZE]);

        YieldNow(false).await;

        CommandError::Custom(format!("failed with {}", buffer[0]))
    }
}

#[cli::program(async)]
enum MyCli {
    SmallCommand(SmallCommand),
    LargeCommand(LargeCommand),
    LargerCommand(LargerCommand),
}

fn env(args: &[&str]) -> Environment {
    Environment::default()
        .with_argv(args.iter().map(|s| s.to_string()).collect())
}

fn parse_command(env: &Environment) -> <MyCli as CliEnums>::Enum {
    let cli
        = MyCli::build_cli().unwrap();

    let clipanion::core::SelectionResult::Command(_, _, command) = MyCli::parse_args(&cli, env).unwrap() else {
        panic!("expected a command");
    };

    <MyCli as CliEnums>::Enum::try_from(command)
        .unwrap()
}

#[test]
fn it_keeps_the_dispatch_future_small() {
    let env
        = env(&["larger"]);

    let command
        = parse_command(&env);

    let future
        = command.execute(&env);

    // Without boxing, the dispatch future embeds the largest command future
    // (more than `2 * BUFFER_SIZE` bytes).
    assert!(std::mem::size_of_val(&future) < 1024, "dispatch future is {} bytes", std::mem::size_of_val(&future));

    let result
        = block_on(future);

    assert_eq!(result.exit_code, ExitCode::FAILURE);
    assert!(result.error_message.unwrap().contains("failed with 0"));
}

#[test]
fn it_keeps_the_run_future_small() {
    let future
        = MyCli::run(env(&["large"]));

    assert!(std::mem::size_of_val(&future) < 4096, "run future is {} bytes", std::mem::size_of_val(&future));

    assert_eq!(block_on(future), ExitCode::from(42));
}

#[test]
fn it_runs_async_commands() {
    assert_eq!(block_on(MyCli::run(env(&["small"]))), ExitCode::SUCCESS);
    assert_eq!(block_on(MyCli::run(env(&["larger"]))), ExitCode::FAILURE);
}

#[test]
fn it_keeps_the_futures_send_when_the_commands_are() {
    let env
        = env(&["small"]);

    let future
        = parse_command(&env).execute(&env);

    assert_send(&future);
    assert_send(&MyCli::run(env.clone()));
}

#[cli::command]
#[cli::path("local")]
struct LocalCommand {}

impl LocalCommand {
    async fn execute(&self) {
        let value
            = Rc::new(1);

        YieldNow(false).await;

        assert_eq!(*value, 1);
    }
}

// The commands don't have to be `Send`.
#[cli::program(async)]
enum MyLocalCli {
    LocalCommand(LocalCommand),
}

#[test]
fn it_runs_commands_that_arent_send() {
    assert_eq!(block_on(MyLocalCli::run(env(&["local"]))), ExitCode::SUCCESS);
}
