# v0.1.4

- Fix MSI service recovery configuration by replacing the Windows Installer native failure-actions table with WiX Util service configuration.
- Preserve automatic restart for both Windows services: a five-second delay and a one-day failure-count reset period.
- Automatically install the Util extension matching WiX Toolset v5 during MSI builds.
- Update package versions and installation instructions to 0.1.4.

Validation: 65 Rust tests passed; the Windows debug MSI built successfully and its recovery configuration tables were verified. Actual service installation still requires verification on a target Windows machine with administrator privileges. The MSI is unsigned.
