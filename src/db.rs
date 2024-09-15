use rusqlite::{Connection, Result as SqliteResult, params};
use std::path::PathBuf;
use std::fs;
use std::process::Command;
use dirs;

#[derive(Debug, Clone)]
pub struct Account {
    pub email: String,
    pub name: String,
    pub is_active: bool,
}

pub struct SSHKey {
    pub email: String,
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
}

pub fn get_db_path() -> PathBuf {
    let home_dir = dirs::home_dir().unwrap();
    home_dir.join(".git_ledger_tui.db")
}

pub fn get_ssh_dir() -> PathBuf {
    let home_dir = dirs::home_dir().unwrap();
    home_dir.join(".ssh")
}

pub fn init_db() -> SqliteResult<()> {
    let conn = Connection::open(get_db_path())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS accounts (
            email TEXT PRIMARY KEY,
            name TEXT,
            is_active INTEGER
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ssh_keys (
            email TEXT PRIMARY KEY,
            private_key BLOB,
            public_key BLOB,
            FOREIGN KEY(email) REFERENCES accounts(email) ON DELETE CASCADE
        )",
        [],
    )?;
    
    Ok(())
}

pub fn list_accounts() -> SqliteResult<Vec<Account>> {
    let conn = Connection::open(get_db_path())?;
    let mut stmt = conn.prepare("SELECT name, email, is_active FROM accounts")?;
    let accounts = stmt.query_map([], |row| {
        Ok(Account {
            name: row.get(0)?,
            email: row.get(1)?,
            is_active: row.get::<_, i32>(2)? == 1,
        })
    })?
    .collect::<Result<Vec<_>, _>>()?;
    Ok(accounts)
}

pub fn add_account(name: &str, email: &str) -> Result<String, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;

    conn.execute("UPDATE accounts SET is_active = 0", []).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO accounts (email, name, is_active) VALUES (?, ?, 1)",
        params![email, name],
    ).map_err(|e| e.to_string())?;

    let (private_key, public_key) = generate_ssh_key(email)?;

    conn.execute(
        "INSERT OR REPLACE INTO ssh_keys (email, private_key, public_key) VALUES (?, ?, ?)",
        params![email, &private_key, &public_key],
    ).map_err(|e| e.to_string())?;

    run_git_command(&["config", "--global", "user.name", name])?;
    run_git_command(&["config", "--global", "user.email", email])?;

    Ok(String::from_utf8(public_key).map_err(|e| e.to_string())?)
}

pub fn remove_account(email: &str) -> Result<(), String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;

    let is_active: bool = conn.query_row(
        "SELECT is_active FROM accounts WHERE email = ?",
        params![email],
        |row| row.get(0)
    ).unwrap_or(false);

    conn.execute("DELETE FROM accounts WHERE email = ?", params![email]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM ssh_keys WHERE email = ?", params![email]).map_err(|e| e.to_string())?;

    if is_active {
        run_git_command(&["config", "--global", "--unset", "user.name"])?;
        run_git_command(&["config", "--global", "--unset", "user.email"])?;
        
        let ssh_dir = get_ssh_dir();
        fs::remove_file(ssh_dir.join("id_ed25519")).ok();
        fs::remove_file(ssh_dir.join("id_ed25519.pub")).ok();
    }

    Ok(())
}

pub fn switch_account(email: &str) -> Result<(), String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;

    conn.execute("UPDATE accounts SET is_active = 0", []).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE accounts SET is_active = 1 WHERE email = ?",
        params![email],
    ).map_err(|e| e.to_string())?;

    let name: String = conn.query_row(
        "SELECT name FROM accounts WHERE email = ?",
        params![email],
        |row| row.get(0)
    ).map_err(|e| e.to_string())?;

    set_active_ssh_key(email)?;

    run_git_command(&["config", "--global", "user.name", &name])?;
    run_git_command(&["config", "--global", "user.email", email])?;

    Ok(())
}

fn generate_ssh_key(email: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let ssh_dir = get_ssh_dir();
    fs::create_dir_all(&ssh_dir).map_err(|e| e.to_string())?;

    let private_key_path = ssh_dir.join("id_ed25519");
    let public_key_path = ssh_dir.join("id_ed25519.pub");

    fs::remove_file(&private_key_path).ok();
    fs::remove_file(&public_key_path).ok();

    Command::new("ssh-keygen")
        .args(&["-t", "ed25519", "-f", private_key_path.to_str().unwrap(), "-N", "", "-C", email])
        .output()
        .map_err(|e| e.to_string())?;

    let private_key = fs::read(&private_key_path).map_err(|e| e.to_string())?;
    let public_key = fs::read(&public_key_path).map_err(|e| e.to_string())?;

    Ok((private_key, public_key))
}

fn set_active_ssh_key(email: &str) -> Result<(), String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let (private_key, public_key): (Vec<u8>, Vec<u8>) = conn.query_row(
        "SELECT private_key, public_key FROM ssh_keys WHERE email = ?",
        params![email],
        |row| Ok((row.get(0)?, row.get(1)?))
    ).map_err(|e| e.to_string())?;

    let ssh_dir = get_ssh_dir();
    fs::write(ssh_dir.join("id_ed25519"), private_key).map_err(|e| e.to_string())?;
    fs::write(ssh_dir.join("id_ed25519.pub"), public_key).map_err(|e| e.to_string())?;

    Ok(())
}

pub fn get_ssh_key(email: &str) -> Result<String, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let public_key: Vec<u8> = conn.query_row(
        "SELECT public_key FROM ssh_keys WHERE email = ?",
        params![email],
        |row| row.get(0)
    ).map_err(|e| e.to_string())?;
    
    String::from_utf8(public_key).map_err(|e| e.to_string())
}

fn run_git_command(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn get_current_user() -> Result<Account, String> {
    let name = run_git_command(&["config", "--global", "user.name"])?;
    let email = run_git_command(&["config", "--global", "user.email"])?;

    if name.is_empty() || email.is_empty() {
        return Err("No current user found".to_string());
    }

    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let is_active: bool = conn.query_row(
        "SELECT is_active FROM accounts WHERE email = ?",
        params![&email],
        |row| row.get(0)
    ).unwrap_or(false);

    Ok(Account {
        name,
        email,
        is_active,
    })
}