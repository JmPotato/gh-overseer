use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use log::debug;
use octocrab::models::{self, pulls::ReviewState};

use crate::config::Config;

#[derive(Debug)]
pub struct Stats {
    // Issues of each user has created.
    issues: HashMap<String, u64>,
    // PRs of each user has created.
    prs: HashMap<String, u64>,
    // Issue comments of each user has given.
    issue_comments: HashMap<String, u64>,
    // PR review comments of each user has given.
    pr_reviews: HashMap<String, u64>,
    // LGTMs of each user has given.
    lgtms: HashMap<String, u64>,
    // labels of each user has added.
    labels: HashMap<String, u64>,

    // The allow list of users.
    allowed_users: HashSet<String>,
    // The allow list of LGTM comments.
    lgtm_comments: Vec<String>,
    // The start time of the stats.
    start_time: DateTime<Utc>,
    // The end time of the stats.
    end_time: DateTime<Utc>,
}

impl Stats {
    pub fn new(config: &Config, start_time: DateTime<Utc>, end_time: DateTime<Utc>) -> Self {
        let allowed_users: HashSet<String> = config.review_users().into_iter().collect();
        Self {
            issues: HashMap::with_capacity(allowed_users.len()),
            prs: HashMap::with_capacity(allowed_users.len()),
            issue_comments: HashMap::with_capacity(allowed_users.len()),
            pr_reviews: HashMap::with_capacity(allowed_users.len()),
            lgtms: HashMap::with_capacity(allowed_users.len()),
            labels: HashMap::with_capacity(allowed_users.len()),
            allowed_users,
            lgtm_comments: config.review_lgtm_comments(),
            start_time,
            end_time,
        }
    }

    /// Traverse the issues (including PRs) to collect the PRs and issues created by each user.
    pub fn traverse_issues(&mut self, issues: Vec<models::issues::Issue>) {
        issues.iter().for_each(|issue| {
            if self.filter_issues(issue) {
                return;
            }
            match issue.pull_request {
                Some(_) => {
                    debug!("traverse pull request: {}", issue_into_string(issue));
                    self.add_pr(&issue.user.login)
                }
                None => {
                    debug!("traverse issue: {}", issue_into_string(issue));
                    self.add_issue(&issue.user.login)
                }
            }
        })
    }

    /// Traverse the issue comments to collect the issue comments given by each user.
    pub fn traverse_issue_comments(&mut self, issue_comments: Vec<models::issues::Comment>) {
        issue_comments.iter().for_each(|comment| {
            if self.filter_issue_comment(comment) {
                return;
            }
            debug!(
                "traverse issue comment: {}",
                issue_comment_into_string(comment)
            );
            self.add_issue_comment(&comment.user.login)
        })
    }

    /// Traverse the PR comments to collect the PR reviews given by each user.
    pub fn traverse_pull_request_comments(
        &mut self,
        pull_request_comments: Vec<models::pulls::Comment>,
    ) {
        pull_request_comments.iter().for_each(|comment| {
            if self.filter_pull_request_comment(comment) {
                return;
            }
            let user = comment.user.as_ref().map_or("", |auth| &auth.login);
            debug!(
                "traverse pull request comment: #{} {:?} by {}",
                comment.id, comment.body, user
            );
            if self.is_comment_lgtm(comment.body.trim()) {
                self.add_lgtm(user)
            } else {
                self.add_pr_review(user)
            }
        })
    }

    /// Traverse the PR reviews to collect the PR approvals given by each user.
    pub fn traverse_pull_request_reviews(&mut self, reviews: Vec<models::pulls::Review>) {
        reviews.iter().for_each(|review| {
            if self.filter_pull_request_review(review) {
                return;
            }
            let user = review.user.as_ref().map_or("", |auth| &auth.login);
            debug!(
                "traverse pull request review: #{} [{:?}] {:?} by {}",
                review.id, review.state, review.body, user
            );
            if let Some(state) = review.state {
                match state {
                    ReviewState::Approved => self.add_lgtm(user),
                    _ => {}
                }
            }
        })
    }

    /// Consume and merge the other stats into self.
    pub fn merge(&mut self, other: Self) {
        Self::merge_map(&mut self.issues, &other.issues);
        Self::merge_map(&mut self.prs, &other.prs);
        Self::merge_map(&mut self.issue_comments, &other.issue_comments);
        Self::merge_map(&mut self.pr_reviews, &other.pr_reviews);
        Self::merge_map(&mut self.lgtms, &other.lgtms);
        Self::merge_map(&mut self.labels, &other.labels);
    }

    fn within_time_range(&self, date_time: DateTime<Utc>) -> bool {
        self.start_time <= date_time && date_time <= self.end_time
    }

    fn filter_issues(&self, issue: &models::issues::Issue) -> bool {
        let user_allowed = self.is_user_allowed(&issue.user.login);
        let within_time_range = self.within_time_range(issue.created_at);
        debug!(
            "filter issue {} [user_allowed]: {}, [crated_at {} within_time_range] {}",
            issue_into_string(issue),
            user_allowed,
            issue.created_at,
            within_time_range
        );
        !user_allowed || !within_time_range
    }

    fn filter_issue_comment(&self, comment: &models::issues::Comment) -> bool {
        let user_allowed = self.is_user_allowed(&comment.user.login);
        let within_time_range = self.within_time_range(comment.created_at)
            || comment
                .updated_at
                .map_or(false, |updated_at| self.within_time_range(updated_at));
        debug!(
            "filter issue comment {} [user_allowed]: {}, [created_at {} updated_at {:?} within_time_range] {}",
            issue_comment_into_string(comment),
            user_allowed,
            comment.created_at,
            comment.updated_at,
            within_time_range
        );
        !user_allowed || !within_time_range
    }

    fn filter_pull_request_comment(&self, comment: &models::pulls::Comment) -> bool {
        let user = comment.user.as_ref().map_or("", |auth| &auth.login);
        let user_allowed = self.is_user_allowed(user);
        let within_time_range = self.within_time_range(comment.created_at)
            || self.within_time_range(comment.updated_at);
        debug!(
            "filter pull request comment {} [user_allowed]: {}, [created_at {} updated_at {:?} within_time_range] {}",
            pull_comment_into_string(comment),
            user_allowed,
            comment.created_at,
            comment.updated_at,
            within_time_range
        );
        !user_allowed || !within_time_range
    }

    fn filter_pull_request_review(&self, review: &models::pulls::Review) -> bool {
        let user = review.user.as_ref().map_or("", |auth| &auth.login);
        let user_allowed = self.is_user_allowed(user);
        let within_time_range = review
            .submitted_at
            .map_or(false, |submitted_at| self.within_time_range(submitted_at));
        debug!(
            "filter pull request review {} [user_allowed]: {}, [submitted_at {:?} within_time_range] {}",
            pull_review_into_string(review),
            user_allowed,
            review.submitted_at,
            within_time_range
        );
        !user_allowed || !within_time_range
    }

    #[inline(always)]
    fn is_user_allowed(&self, user: &str) -> bool {
        self.allowed_users.contains(user)
    }

    #[inline(always)]
    fn is_comment_lgtm(&self, comment: &str) -> bool {
        self.lgtm_comments.iter().any(|lgtm| comment.contains(lgtm))
    }

    #[inline(always)]
    fn add_issue(&mut self, user: &str) {
        let count = self.issues.entry(user.to_string()).or_insert(0);
        *count += 1;
    }

    #[inline(always)]
    fn add_pr(&mut self, user: &str) {
        let count = self.prs.entry(user.to_string()).or_insert(0);
        *count += 1;
    }

    #[inline(always)]
    fn add_issue_comment(&mut self, user: &str) {
        let count = self.issue_comments.entry(user.to_string()).or_insert(0);
        *count += 1;
    }

    #[inline(always)]
    fn add_pr_review(&mut self, user: &str) {
        let count = self.pr_reviews.entry(user.to_string()).or_insert(0);
        *count += 1;
    }

    #[inline(always)]
    fn add_lgtm(&mut self, user: &str) {
        let count = self.lgtms.entry(user.to_string()).or_insert(0);
        *count += 1;
    }

    #[inline(always)]
    fn add_label(&mut self, user: &str) {
        let count = self.labels.entry(user.to_string()).or_insert(0);
        *count += 1;
    }

    #[inline(always)]
    fn merge_map(base: &mut HashMap<String, u64>, added: &HashMap<String, u64>) {
        for (user, delta) in added {
            let count = base.entry(user.to_string()).or_insert(0);
            *count += *delta;
        }
    }
}

#[inline(always)]
fn issue_into_string(issue: &models::issues::Issue) -> String {
    format!(
        "issue: #{} [{:?}] {:?} by {}",
        issue.number, issue.state, issue.title, issue.user.login
    )
}

#[inline(always)]
fn issue_comment_into_string(comment: &models::issues::Comment) -> String {
    format!(
        "issue comment: #{} {:?} by {}",
        comment.id, comment.body, comment.user.login
    )
}

#[inline(always)]
fn pull_comment_into_string(comment: &models::pulls::Comment) -> String {
    format!(
        "pull request comment: #{} {:?} by {}",
        comment.id,
        comment.body,
        comment.user.as_ref().map_or("", |auth| &auth.login)
    )
}

#[inline(always)]
fn pull_review_into_string(review: &models::pulls::Review) -> String {
    format!(
        "pull request review: #{} {:?} by {}",
        review.id,
        review.state,
        review.user.as_ref().map_or("", |auth| &auth.login)
    )
}
