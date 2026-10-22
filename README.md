# awcc-lite 👽⚡

> **An ultra-lightweight, bloat-free Alienware Command Center replacement written in 100% pure Rust.**  
> Native Win32 + GDI+ GUI with zero Electron, zero WebViews, zero Node.js, and zero background telemetries.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011%20(x64)-brightgreen.svg)]()
[![Language: Rust](https://img.shields.io/badge/Language-Rust%202024-orange.svg)]()
[![Hardware: Alienware 16 Aurora AC16250](https://img.shields.io/badge/Tested%20on-Alienware%2016%20Aurora%20(AC16250)-cyan.svg)]()

---

## 🚀 Why awcc-lite?

The official Dell Alienware Command Center (AWCC) installs dozens of background services, telemetry subagents, and memory-hogging dependencies that easily consume **1.5 GB to 2.0 GB of RAM** while introducing micro-stutters and idle battery drain.

`awcc-lite` communicates directly with your laptop's **ACPI BIOS WMI bridge (`AWCCWmiMethodFunction`)**, providing instant fan, thermal, TCC limit, and macro controls in a tiny **< 1 MB standalone executable**.

| Metric | Official AWCC (v6.x) | awcc-lite |
| :--- | :--- | :--- |
| **RAM Usage** | ~1,200 MB – 2,000 MB | **~8 MB – 14 MB** |
| **Background Processes** | 15+ subagents & services | **0 background services** |
| **Executable Size** | > 1.2 GB installation footprint | **~500 KB single .exe** |
| **GUI Framework** | Heavy Web / WinUI / Node stack | **Pure Win32 + Hardware GDI+** |
| **Startup Time** | 8 – 20 seconds | **< 100 milliseconds** |
| **Telemetry & Tracking** | TechHub, SupportAssist, Analytics | **Zero tracking / Zero network calls** |

---

## 🎯 Hardware Compatibility

### 🔬 Verified Hardware
* **Model:** **Alienware 16 Aurora (AC16250)**
* **Status:** **100% Tested & Verified**
  * ✅ Thermal Profile Switching (Battery, Quiet, Balanced, Performance, Overdrive)
  * ✅ CPU Thermal Throttle Limit (TCC Offset: 0°C to 25°C)
  * ✅ Real-time Tachometer Fan RPM (CPU & GPU fans)
  * ✅ Dedicated Keyboard Macros (M1, M2, M3 on F1, F2, F3)
  * ✅ Global Profile Cycle Hotkey (`F6`)

### 🌐 Community Hardware Compatibility
`awcc-lite` dynamically queries the firmware device description (`Thermal_Information / OP_DESCRIBE`) via the standard Dell `AWCCWmiMethodFunction` interface. It is architecturally compatible with modern Alienware and Dell gaming systems:
* **Alienware Laptops:** m15, m16, m17, m18, x14, x15, x16, x17
* **Alienware Desktops:** Aurora R13, R14, R15, R16
* **Dell Gaming Laptops:** G15, G16 (supporting G-Mode and standard profiles)

---

## ✨ Features

- **⚡ Instant Thermal Switching:** Seamless toggle between firmware-level modes:
  - `Battery (0xA2)` – Maximum energy conservation
  - `Quiet (0xA3)` – Whisper-silent 0 dB fan curves
  - `Balanced (0xA0)` – Optimal everyday performance
  - `Performance (0xA1)` – Boosted clock speeds & fan curves
  - `Overdrive (0xA4)` – 100% turbo power limit & maximum cooling
- **🛡️ Stealth Mode:** Instant single-click silent profile toggle with previous state memory.
- **🌡️ CPU Thermal Limit (TCC):** Custom TJMax offset control (e.g. limit max temperature to 85°C, 90°C, or 95°C) to prevent CPU thermal throttling without third-party undervolt tools.
- **💨 Real-Time Cooling Tachometers:** Live CPU & GPU fan RPM readout, capacity percentage, and passive 0 dB indicators.
- **⌨️ Dedicated Keyboard Macros (M1 / M2 / M3):**
  - Mapped directly to hardware keys (`F1`, `F2`, `F3`).
  - Assignable to Turbo Overdrive, Whisper Quiet, Max Cooling, Task Manager, or Balanced.
  - Full **Unassign ("None / Kaldır")** support to restore standard Windows F1–F3 behavior.
- **🔄 F6 Hotkey:** Cycle through thermal modes instantly on the fly while inside games.
- **🧹 Auto Conflict Resolution:** Automatically detects and cleanly terminates lingering Dell background subagents (`AWPerformance.SCSubAgent`, `AlienFXSubAgent`, etc.) on launch to prevent Dell from overwriting your thermal profiles.

---

## 📥 Installation & Running

### Requirements
* **Operating System:** Windows 10 (64-bit) or Windows 11 (64-bit)
* **Privileges:** Administrator access (required by Windows to send ACPI WMI write commands)

### Running
1. Download `awcc-lite.exe` from the latest release.
2. Double click to run. Windows UAC will prompt for Administrator permission.
3. Keep it open or minimize it when adjusting profiles.

---

## 🛠️ Building from Source

Ensure you have Rust and Cargo installed:
```powershell
# Clone the repository
git clone https://github.com/ardaozyildirim/awcc-rust.git
cd awcc-rust

# Build optimized release executable
cargo build --release

# The compiled binary will be located at:
# target/release/awcc-lite.exe
```

---

## 🧹 Cleaning Up Dell Bloatware (Optional)

The repository includes standalone scripts to pause or permanently disable all background Dell telemetry services:

* **Disable all Dell Services & Subagents:**
  ```powershell
  # Right click and run as Administrator:
  .\disable-all-dell-services.cmd
  ```
* **Launch Official Dell AWCC Uninstaller:**
  ```powershell
  .\uninstall-awcc.cmd
  ```

---

## ⚖️ License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for more information.

---

## ⚠️ Disclaimer

`awcc-lite` is an independent open-source project and is not affiliated with, endorsed by, or sponsored by Dell Inc. or Alienware Corporation. All trademarks and brand names belong to their respective owners.
