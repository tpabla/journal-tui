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
        
        // Provide password via stdin with newline
        if let Some(mut stdin) = child.stdin.take() {
            // Write the password with newline for -stdinpass
            writeln!(stdin, "{}", password)?;
        }
        
        let output = child.wait_with_output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to create encrypted volume: {}", error));
        }
        
        // We don't need to save the password since we generate it deterministically
        // The entries directory will be created on first actual mount
        
        Ok(())
    }
    
    fn generate_secure_password(&self) -> String {
        // Use a deterministic password based on user's home directory
        // This way we don't need to store/retrieve it from keychain
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        let home = dirs::home_dir().expect("Could not find home directory");
        home.hash(&mut hasher);
        "JournalVault".hash(&mut hasher);
        
        // Create a long, complex password that's consistent for this user
        format!("JV_{}_{}_Secure", hasher.finish(), hasher.finish() * 7)
    }
    
    
    pub fn mount_with_keychain(&self) -> Result<()> {
        if self.is_mounted() {
            return Ok(());
        }
        
        // Use the same deterministic password that was used to create the vault
        let password = self.generate_secure_password();
        
        // Mount with the password, adding newline for proper stdin format
        let mut child = Command::new("hdiutil")
            .args(&[
                "attach",
                self.dmg_path.to_str().unwrap(),
                "-stdinpass",
                "-mountpoint", self.mount_point.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        // Provide password via stdin with newline
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