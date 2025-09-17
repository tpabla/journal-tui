use anyhow::Result;

#[cfg(target_os = "macos")]
pub fn authenticate() -> Result<bool> {
    use localauthentication_rs::{LocalAuthentication, LAPolicy};
    
    let auth = LocalAuthentication::new();
    
    // Try Touch ID first, if not available fall back to password/watch
    let authenticated = auth.evaluate_policy(
        LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
        "Access your private journal entries"
    );
    
    if !authenticated {
        // Try with fallback to password if Touch ID failed
        let authenticated_fallback = auth.evaluate_policy(
            LAPolicy::DeviceOwnerAuthentication,
            "Access your private journal entries"
        );
        
        Ok(authenticated_fallback)
    } else {
        Ok(true)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn authenticate() -> Result<bool> {
    // On non-macOS systems, just return true (no authentication)
    Ok(true)
}