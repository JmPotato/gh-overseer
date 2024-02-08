mod config;
mod fetcher;
mod stats;

use std::process;

use chrono::{DateTime, Utc};
use clap::Parser;
use log::{error, info, warn};
use octocrab::Octocrab;
use tokio::sync::mpsc::unbounded_channel;

use crate::config::Config;
use crate::fetcher::Fetcher;
use crate::stats::Stats;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the configuration file.
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Log level. Should be the following values:
    ///   - error
    ///   - warn
    ///   - info
    ///   - debug
    ///   - trace
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Start time should be in the RFC3339 format like "2015-09-21T00:00:00Z".
    #[arg(short, long, required = true)]
    start_time: String,

    /// End time should be in the RFC3339 format like "2015-09-21T00:00:00Z".
    #[arg(short, long, required = false)]
    end_time: Option<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    init_logger(&args.log_level);

    // TODO: support both configuration file and command line arguments.
    let config = Config::load(&args.config).unwrap_or_else(|err| {
        error!("failed to load config file from '{}': {}", args.config, err);
        process::exit(1);
    });
    info!("config loaded from {}", args.config);

    let octocrab = Octocrab::builder()
        .personal_token(config.github_personal_token())
        .build()
        .unwrap_or_else(|err| {
            error!("failed to build github client instance: {}", err);
            process::exit(1);
        });
    info!("github client instance built");

    let repos = config.review_repos();
    let (start_time, end_time) = (
        match DateTime::parse_from_rfc3339(&args.start_time) {
            Ok(start_time) => start_time.to_utc(),
            Err(err) => {
                error!("failed to parse start time '{}': {}", args.start_time, err);
                process::exit(1);
            }
        },
        if let Some(end_time) = &args.end_time {
            match DateTime::parse_from_rfc3339(&end_time) {
                Ok(end_time) => end_time.to_utc(),
                Err(err) => {
                    error!("failed to parse end time '{}': {}", end_time, err);
                    process::exit(1);
                }
            }
        } else {
            Utc::now()
        },
    );
    info!("time range: {} ~ {}", start_time, end_time);

    let (tx, mut rx) = unbounded_channel();
    let mut tasks = Vec::new();
    for repo in repos {
        let octocrab = octocrab.clone();
        let fetcher = Fetcher::new(octocrab, &repo, start_time).unwrap_or_else(|err| {
            error!("failed to init fetcher for '{}': {}", repo, err);
            process::exit(1);
        });
        let mut stats = Stats::new(&config, start_time, end_time);
        let tx = tx.clone();

        tasks.push((
            repo.clone(),
            tokio::spawn(async move {
                // Fetch all issues and PRs.
                let issues_and_prs = match fetcher.fetch_issues().recv().await {
                    Some(issues_and_prs) => {
                        stats.traverse_issues(issues_and_prs.clone());
                        issues_and_prs
                    }
                    None => {
                        warn!("no issues and pull requests fetched for '{}'", repo);
                        return;
                    }
                };

                // Fetch all comments for issues and PRs.
                let mut issue_comments_rx = fetcher.fetch_issue_comments(
                    issues_and_prs
                        .iter()
                        .filter(|issue| issue.pull_request.is_none())
                        .map(|issue| issue.number)
                        .collect(),
                );
                let mut pull_request_comments_rx = fetcher.fetch_pull_request_comments();

                // Fetch all reviews for PRs.
                let pull_requests = issues_and_prs
                    .iter()
                    .filter(|issue| issue.pull_request.is_some())
                    .map(|pull_request| pull_request.number);
                let mut pull_request_reviews_rx =
                    fetcher.fetch_pull_request_reviews(pull_requests.collect());

                // Wait for the fetcher to finish fetching all data.
                if let Some(issue_comments) = issue_comments_rx.recv().await {
                    stats.traverse_issue_comments(issue_comments);
                }
                if let Some(pull_request_comments) = pull_request_comments_rx.recv().await {
                    stats.traverse_pull_request_comments(pull_request_comments);
                }
                if let Some(pull_request_reviews) = pull_request_reviews_rx.recv().await {
                    stats.traverse_pull_request_reviews(pull_request_reviews);
                }
                // Send back the stats to the main thread.
                tx.send(stats).unwrap_or_else(|err| {
                    error!(
                        "failed to send stats back to the main thread for '{}': {}",
                        repo, err
                    );
                    return;
                });
            }),
        ));
    }
    // Wait for all tasks to finish.
    for (repo, task) in tasks {
        task.await
            .unwrap_or_else(|err| error!("failed to finish task for '{}': {}", repo, err));
    }
    drop(tx);

    // Merge all stats from the tasks.
    let mut stats: Option<Stats> = None;
    loop {
        match rx.recv().await {
            Some(s) => {
                if let Some(ref mut stats) = stats {
                    stats.merge(s);
                } else {
                    stats = Some(s);
                }
            }
            None => break,
        }
    }
    match stats {
        Some(stats) => info!("all stats merged: {:?}", stats),
        None => info!("no stats generated at all"),
    }
}

fn init_logger(log_level: &str) {
    let mut builder = env_logger::Builder::from_default_env();
    builder
        .target(env_logger::Target::Stdout)
        .filter_level(get_log_level(log_level))
        .init();
}

fn get_log_level(log_level: &str) -> log::LevelFilter {
    match log_level {
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        // Use info as the default log level.
        _ => log::LevelFilter::Info,
    }
}
