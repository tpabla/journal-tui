use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::fs;
use std::io::Write;

#[derive(Clone)]
pub struct VolumeManager {
    dmg_path: PathBuf,
    volume_name: String,
    mount_point: PathBuf,
}

impl VolumeManager {
    pub fn new() -> Self {
        let home_dir = dirs::home_dir().expect("Could not find home directory");
        let dmg_path = home_dir.join(".journal").join("vault.dmg");
        let volume_name = "JournalVault".to_string();
        let mount_point = PathBuf::from("/Volumes").join(&volume_name);
        
        Self {
            dmg_path,
            volume_name,
            mount_point,
        }
    }
    
    pub fn dmg_exists(&self) -> bool {
        self.dmg_path.exists()
    }
    
    pub fn is_mounted(&self) -> bool {
        self.mount_point.exists()
    }
    
    pub fn get_entries_path(&self) -> PathBuf {
        self.mount_point.join("entries")
    }
    
    pub fn create_encrypted_volume(&self) -> Result<()> {
        // Generate a secure random password for the volume
        let password = self.generate_secure_password();
        // Ensure parent directory exists
        if let Some(parent) = self.dmg_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Create encrypted DMG with hdiutil using stdinpass to avoid interactive prompt
        let mut child = Command::new("hdiutil")
            .args(&[
                "create",
                "-size", "100m",
                "-fs", "APFS",
                "-encryption", "AES-256",
                "-stdinpass",  // Read password from stdin without prompting
                "-volname", &self.volume_name,
                self.dmg_path.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        // Provide password via stdin
        if let Some(mut stdin) = child.stdin.take() {
            // Just write the password once with -stdinpass
            write!(stdin, "{}", password)?;
            stdin.flush()?;
        }
        
        let output = child.wait_with_output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to create encrypted volume: {}", error));
        }
        
        // Mount the volume to create entries directory
        self.mount_with_password(&password)?;
        
        // Create entries directory
        let entries_path = self.get_entries_path();
        fs::create_dir_all(&entries_path)?;
        
        // Unmount after setup
        self.unmount()?;
        
        // Save password to keychain for Touch ID access
        self.save_password_to_keychain(&password)?;
        
        Ok(())
    }
    
    fn generate_secure_password(&self) -> String {
        use rand::Rng;
        use rand::distributions::Alphanumeric;
        
        // Generate a secure 32-character random password
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect()
    }
    
    pub fn mount_with_password(&self, password: &str) -> Result<()> {
        if self.is_mounted() {
            return Ok(());
        }
        
        let mut child = Command::new("hdiutil")
            .args(&[
                "attach",
                self.dmg_path.to_str().unwrap(),
                "-stdinpass",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        // Provide password via stdin
        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{}", password)?;
        }
        
        let output = child.wait_with_output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to mount volume: {}", error));
        }
        
        Ok(())
    }
    
    pub fn mount_with_keychain(&self) -> Result<()> {
        if self.is_mounted() {
            return Ok(());
        }
        
        // Try to get password from keychain
        let password = self.get_password_from_keychain()?;
        self.mount_with_password(&password)
    }
    
    pub fn unmount(&self) -> Result<()> {
        if !self.is_mounted() {
            return Ok(());
        }
        
        let output = Command::new("hdiutil")
            .args(&[
                "detach",
                self.mount_point.to_str().unwrap(),
            ])
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            // Force unmount if regular unmount fails
            let force_output = Command::new("hdiutil")
                .args(&[
                    "detach",
                    self.mount_point.to_str().unwrap(),
                    "-force",
                ])
                .output()?;
            
            if !force_output.status.success() {
                return Err(anyhow!("Failed to unmount volume: {}", error));
            }
        }
        
        Ok(())
    }
    
    pub fn save_password_to_keychain(&self, password: &str) -> Result<()> {
        // Use security command to add password to keychain
        let output = Command::new("security")
            .args(&[
                "add-generic-password",
                "-a", "journal-tui",
                "-s", "JournalVault",
                "-w", password,
                "-U",  // Update if exists
                "-T", "/usr/bin/security",  // Allow security command to access
            ])
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to save password to keychain: {}", error));
        }
        
        Ok(())
    }
    
    pub fn get_password_from_keychain(&self) -> Result<String> {
        let output = Command::new("security")
            .args(&[
                "find-generic-password",
                "-a", "journal-tui",
                "-s", "JournalVault",
                "-w",  // Print only the password
            ])
            .output()?;
        
        if !output.status.success() {
            return Err(anyhow!("Password not found in keychain"));
        }
        
        let password = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();
        
        Ok(password)
    }
    
    pub fn migrate_entries(&self, source_dir: &Path) -> Result<usize> {
        if !self.is_mounted() {
            return Err(anyhow!("Volume must be mounted before migration"));
        }
        
        let dest_dir = self.get_entries_path();
        fs::create_dir_all(&dest_dir)?;
        
        let mut count = 0;
        if source_dir.exists() {
            for entry in fs::read_dir(source_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    let file_name = path.file_name().unwrap();
                    let dest_path = dest_dir.join(file_name);
                    
                    fs::copy(&path, &dest_path)?;
                    count += 1;
                }
            }
        }
        
        Ok(count)
    }
}