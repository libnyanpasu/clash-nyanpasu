use super::JobExt;
use crate::{
    config::ConfigService,
    core::tasks::{
        executor::{AsyncJobExecutor, TaskExecutor},
        task::TaskSchedule,
    },
    utils::dirs,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Local, TimeZone};
use std::{
    fs::{self, DirEntry},
    str::FromStr,
    time::Duration,
};

const CLEAR_LOG_TASK_NAME: &str = "clear_logs";

#[derive(Clone, Default)]
pub struct ClearLogsJob;

/// Clear logs from the logs directory
pub fn clear_logs() -> Result<()> {
    let log_dir = dirs::app_logs_dir()?;
    if !log_dir.exists() {
        return Ok(());
    }

    let minutes = {
        let verge = ConfigService::verge();
        let verge = verge.data();
        #[allow(deprecated)]
        verge.auto_log_clean.unwrap_or(0)
    };
    if minutes == 0 {
        return Ok(()); // 0 means disable
    }
    log::debug!(target: "app", "try to delete log files, minutes: {minutes}");

    // %Y-%m-%d to NaiveDateTime
    let parse_time_str = |s: &str| {
        let sa: Vec<&str> = s.split('-').collect();
        if sa.len() != 4 {
            return Err(anyhow::anyhow!("invalid time str"));
        }

        let year = i32::from_str(sa[0])?;
        let month = u32::from_str(sa[1])?;
        let day = u32::from_str(sa[2])?;
        let time = chrono::NaiveDate::from_ymd_opt(year, month, day)
            .ok_or(anyhow::anyhow!("invalid time str"))?
            .and_hms_opt(0, 0, 0)
            .ok_or(anyhow::anyhow!("invalid time str"))?;
        Ok(time)
    };

    let process_file = |file: DirEntry| -> Result<()> {
        let file_name = file.file_name();
        let file_name = file_name.to_str().unwrap_or_default();

        if file_name.ends_with(".log") {
            let now = Local::now();
            let created_time = parse_time_str(&file_name[0..file_name.len() - 4])?;
            let created_time: DateTime<Local> = Local.from_local_datetime(&created_time).unwrap(); // It is safe to use `unwrap` here because we just parsed it

            let duration = now.signed_duration_since(created_time);
            if duration.num_minutes() > minutes {
                let file_path = file.path();
                let _ = fs::remove_file(file_path);
                log::info!(target: "app", "delete log file: {file_name}");
            }
        }
        Ok(())
    };

    for file in fs::read_dir(&log_dir)? {
        match file {
            Ok(file) => {
                let _ = process_file(file);
            }
            Err(err) => {
                log::error!(target: "app", "read log dir error: {err:?}");
            }
        }
    }
    Ok(())
}

#[async_trait]
impl AsyncJobExecutor for ClearLogsJob {
    async fn execute(&self) -> Result<()> {
        clear_logs()
    }
}

impl JobExt for ClearLogsJob {
    fn name(&self) -> &'static str {
        CLEAR_LOG_TASK_NAME
    }

    fn setup(&self) -> Option<crate::core::tasks::task::Task> {
        Some(crate::core::tasks::task::Task {
            name: CLEAR_LOG_TASK_NAME.to_string(),
            schedule: TaskSchedule::Interval(Duration::from_secs(30 * 60)), // 30 minutes 清理一次
            executor: TaskExecutor::Async(Box::new(self.clone())),
            ..Default::default()
        })
    }
}
