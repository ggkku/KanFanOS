//! SQLite 连接与迁移。
//!
//! 第一版使用单个连接加互斥锁：写入都由后台任务串行完成，
//! 读请求较少，足以满足 1 万到 10 万规模的场景。
//! 如果后续读并发成为瓶颈，再换成连接池或 tokio-rusqlite。

pub mod history;
pub mod libraries;
mod migrations;
pub mod series;
pub mod videos;

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::Connection;

pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建数据库目录失败: {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("打开数据库失败: {}", path.display()))?;

        // WAL 允许读写并发；NORMAL 在 WAL 下兼顾安全性与写入速度。
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        // 遇到写锁时等待而不是立刻返回 SQLITE_BUSY。
        conn.busy_timeout(Duration::from_secs(5))?;

        let db = Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn()?;
        let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let latest = migrations::MIGRATIONS.len() as i64;

        if current > latest {
            bail!("数据库版本 {current} 高于程序支持的版本 {latest}，请升级应用");
        }
        if current == latest {
            return Ok(());
        }

        // 所有未执行的迁移放在同一个事务里，失败时整体回滚。
        let tx = conn.transaction()?;
        for (index, sql) in migrations::MIGRATIONS.iter().enumerate() {
            let version = index as i64 + 1;
            if version > current {
                tx.execute_batch(sql)
                    .with_context(|| format!("执行第 {version} 个数据库迁移失败"))?;
                tx.pragma_update(None, "user_version", version)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 索引中的有效视频数量，不包含已标记为 missing 的记录。
    pub fn video_count(&self) -> Result<i64> {
        Ok(self.conn()?.query_row(
            "SELECT COUNT(*) FROM videos WHERE status <> 'missing'",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn library_count(&self) -> Result<i64> {
        Ok(self
            .conn()?
            .query_row("SELECT COUNT(*) FROM libraries", [], |row| row.get(0))?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow!("数据库连接锁已损坏，请重启应用"))
    }
}
