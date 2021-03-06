#![allow(non_upper_case_globals)]

use async_trait::async_trait;
use celery::error::TaskError;
use celery::task::TaskResult;
use env_logger::Env;
use exitfailure::ExitFailure;
use structopt::StructOpt;
use tokio::time::{self, Duration};

// This generates the task struct and impl with the name set to the function name "add"
#[celery::task]
fn add(x: i32, y: i32) -> TaskResult<i32> {
    Ok(x + y)
}

// Demonstrates a task that raises an error, and also how to customize task options.
// In this case we override the default `max_retries`.
#[celery::task(max_retries = 3)]
fn buggy_task() -> TaskResult<()> {
    Err(TaskError::UnexpectedError(
        "This error is part of the example: it is used to showcase error handling".into(),
    ))
}

// Demonstrates a long running IO-bound task. By increasing the prefetch count, an arbitrary
// number of these number can execute concurrently.
#[celery::task(max_retries = 2)]
async fn long_running_task(secs: Option<u64>) {
    let secs = secs.unwrap_or(10);
    time::delay_for(Duration::from_secs(secs)).await;
}

// Demonstrates a task that is bound to the task instance, i.e. runs as an instance method.
#[celery::task(bind = true)]
fn bound_task(task: &Self) {
    // Print some info about the request for debugging.
    println!("{:?}", task.request.origin);
    println!("{:?}", task.request.hostname);
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "celery_app",
    about = "Run a Rust Celery producer or consumer.",
    setting = structopt::clap::AppSettings::ColoredHelp,
)]
enum CeleryOpt {
    Consume,
    Produce,
}

#[tokio::main]
async fn main() -> Result<(), ExitFailure> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();
    let opt = CeleryOpt::from_args();
    let my_app = celery::app!(
        broker = AMQP { std::env::var("AMQP_ADDR").unwrap_or_else(|_| "amqp://127.0.0.1:5672/my_vhost".into()) },
        tasks = [
            add,
            buggy_task,
            long_running_task,
            bound_task,
        ],
        // This just shows how we can route certain tasks to certain queues based
        // on glob matching.
        task_routes = [
            "buggy_task" => "buggy-queue",
            "*" => "celery",
        ],
        prefetch_count = 2,
        heartbeat = Some(10),
    );

    match opt {
        CeleryOpt::Consume => {
            my_app.consume_from(&["celery", "buggy-queue"]).await?;
        }
        CeleryOpt::Produce => {
            // Basic task sending.
            my_app.send_task(add::new(1, 2)).await?;
            my_app.send_task(bound_task::new()).await?;

            // Sending a task with additional options like `countdown`.
            my_app.send_task(add::new(1, 3).with_countdown(3)).await?;

            // Send the buggy task that will fail and be retried a few times.
            my_app.send_task(buggy_task::new()).await?;

            // Send the long running task that will fail with a timeout error.
            my_app
                .send_task(long_running_task::new(Some(3)).with_time_limit(2))
                .await?;
        }
    };

    my_app.close().await?;

    Ok(())
}
