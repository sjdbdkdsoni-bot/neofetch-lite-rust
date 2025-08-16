use colored::Colorize;
use regex::Regex;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::Command;

#[derive(Debug)]
struct SystemInfo {
    os: Option<String>,
    kernel: Option<String>,
    uptime: Option<String>,
    cpu: Option<String>,
    gpu: Option<String>,
    memory: Option<String>,
    user_hostname: Option<String>,
}

impl SystemInfo {
    fn new() -> Self {
        SystemInfo {
            os: None,
            kernel: None,
            uptime: None,
            cpu: None,
            gpu: None,
            memory: None,
            user_hostname: None,
        }
    }
}

/// Gets the second item in the line after the ':' and trims it accordingly
fn store_proc_info(pointer: &mut String, line: &String) {
    // split the string on the ':', get the second item
    let slices: Vec<&str> = line.split(":").collect();

    // get the string after the : if it exists
    // check that the vector is at least 2 long
    if slices.len() >= 2 {
        *pointer = String::from(slices[1].trim());
    }
}

/// Tries to read CPU frequency from various sources.
/// Returns frequency in GHz as a formatted string (e.g., " @ 3.200GHz") or an empty string if not found.
fn get_cpu_frequency_string() -> String {
    // Method 1: Try /sys/devices/system/cpu/cpu0/cpufreq/bios_limit
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/bios_limit") {
        if let Ok(freq_khz) = content.trim().parse::<u32>() {
            if freq_khz > 0 {
                let freq_ghz = freq_khz as f32 / 1_000_000.0;
                return format!(" @ {:.3}GHz", freq_ghz);
            }
        }
    }

    // Method 2: Try to get max frequency from /proc/cpuinfo (usually the first "cpu MHz" line gives current, we want max)
    // This is a fallback and might be less accurate or represent max frequency correctly.
    if let Ok(proc_file) = File::open("/proc/cpuinfo") {
        let reader = BufReader::new(proc_file);
        for line_result in reader.lines() {
             if let Ok(line) = line_result {
                 // Looking for a line like "cpu MHz : 1200.000"
                 if line.starts_with("cpu MHz") {
                     let parts: Vec<&str> = line.split(':').collect();
                     if parts.len() >= 2 {
                         if let Ok(freq_mhz) = parts[1].trim().parse::<f32>() {
                             // Assume this is a representative frequency, convert to GHz
                             let freq_ghz = freq_mhz / 1000.0;
                             return format!(" @ {:.3}GHz", freq_ghz);
                         }
                     }
                 }
             }
             // Note: We could search for "model name" and try to parse frequency from the string,
             // but that's more complex and error-prone with regex. The above methods are simpler.
        }
    }

    // If no frequency found by any method, return empty string
    "".to_string()
}


/// gather the cpu info from /proc/cpuinfo
/// and tries to get frequency from sysfs or proc
fn get_cpu_info(cpu: &mut Option<String>) {
    let proc_file = File::open("/proc/cpuinfo");
    // Handle error opening /proc/cpuinfo gracefully
    let proc_file = match proc_file {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Warning: Could not open /proc/cpuinfo: {}", e);
            *cpu = Some("Unknown CPU (Error reading /proc/cpuinfo)".to_string());
            return;
        }
    };
    let reader = BufReader::new(proc_file);

    let mut model_name: String = Default::default();
    let mut cpu_cores: String = Default::default();

    for line_result in reader.lines() {
        // Handle potential errors reading lines
         let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Warning: Error reading line from /proc/cpuinfo: {}", e);
                continue; // Skip this line and try the next one
            }
        };

        if model_name.is_empty() && line.contains("model name") {
            store_proc_info(&mut model_name, &line);
        } else if cpu_cores.is_empty() && line.contains("cpu cores") {
            store_proc_info(&mut cpu_cores, &line);
        }

        // break out early if we have the info we need
        if !cpu_cores.is_empty() && !model_name.is_empty() {
            break;
        }
    }

    // Get CPU frequency string using our new helper function
    let cpu_freq_string = get_cpu_frequency_string();

    // Handle cases where model_name or cpu_cores were not found
    if model_name.is_empty() {
        model_name = "Unknown Model".to_string();
    }
    if cpu_cores.is_empty() {
        cpu_cores = "Unknown Cores".to_string();
    }


    // build the final string for the CPU information
    *cpu = Some(format!(
        "{} ({}){}", // Format changed to accommodate optional frequency
        model_name, cpu_cores, cpu_freq_string,
    ));
}

/// gets the GPU info using 'lspci' and formats it, places data string into 'gpu'
fn get_gpu_info(gpu: &mut Option<String>) {
    // Gracefully handle errors from the command
    let command_output_result = Command::new("lspci").output();
    let command_output = match command_output_result {
         Ok(output) => {
             if output.status.success() {
                 String::from_utf8_lossy(&output.stdout).trim().to_string()
             } else {
                  eprintln!("Warning: 'lspci' command failed with status: {}", output.status);
                  "".to_string() // Return empty string if command failed
             }
         },
         Err(e) => {
              eprintln!("Warning: Could not execute 'lspci': {}", e);
              "".to_string() // Return empty string if command couldn't be executed
         }
    };

    // Only proceed if command output is not empty
    if !command_output.is_empty() {
        for line in command_output.lines() {
            if line.contains("VGA compatible controller") || line.contains("3D controller") || line.contains("Display controller") {
                let between_brackets_regex = Regex::new(r"\[([^\]]+)\]");
                // Gracefully handle regex compilation error (shouldn't happen with a static regex)
                let regex_result = between_brackets_regex;
                match regex_result {
                    Ok(re) => {
                        if let Some(captures) = re.captures(line) {
                            *gpu = Some(captures.get(1).map_or("", |m| m.as_str()).to_string());
                            // Break after finding the first relevant GPU
                            break;
                        }
                    },
                    Err(e) => {
                        eprintln!("Warning: Could not compile regex for GPU info: {}", e);
                        // If regex fails, try a simpler extraction
                         if let Some(start) = line.find('[') {
                             if let Some(end) = line[start+1..].find(']') {
                                 *gpu = Some(line[start+1..start+1+end].to_string());
                                 break;
                             }
                         }
                    }
                }
            }
        }
    }

    // Set a default value if no GPU was found
    if gpu.is_none() || gpu.as_ref().unwrap().is_empty() {
         *gpu = Some("No GPU detected".to_string());
    }
}

/// will execute the bash command passed in,
/// returns the stdout as a String, or an empty string on failure
fn send_bash_command(command: &str) -> String {
    let bash_command_process = Command::new(command).output();

    match bash_command_process {
        Ok(output) => {
            if output.status.success() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                eprintln!("Warning: Command '{}' failed with status: {}", command, output.status);
                "".to_string() // Return empty string on command failure
            }
        },
        Err(e) => {
            eprintln!("Warning: Couldn't execute '{}': {}", command, e);
            "".to_string() // Return empty string on execution error
        }
    }
}

/// Copy of bash_command_process, but takes params for the command
/// will execute the bash command passed in with the parameters,
/// returns the stdout as a String, or an empty string on failure
fn send_bash_command_with_params(command: &str, parameters: &[&str]) -> String {
    let bash_command_process = Command::new(command).args(parameters).output();

    match bash_command_process {
        Ok(output) => {
             if output.status.success() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
             } else {
                 eprintln!("Warning: Command '{} {:?}' failed with status: {}", command, parameters, output.status);
                 "".to_string() // Return empty string on command failure
             }
        },
        Err(e) => {
            eprintln!("Warning: Couldn't execute '{} {:?}': {}", command, parameters, e);
            "".to_string() // Return empty string on execution error
        }
    }
}

/// get kernel info using 'uname -r'
fn get_kernel_info(kernel: &mut Option<String>) {
    let kernel_str = send_bash_command_with_params("uname", &["-r"]);
    if kernel_str.is_empty() {
         *kernel = Some("Unknown Kernel".to_string());
    } else {
        *kernel = Some(kernel_str);
    }
}

/// gets the os info
/// will set a default value on fail
fn get_os(os: &mut Option<String>) {
    let os_release_output = send_bash_command_with_params("cat", &["/etc/os-release"]);
    if os_release_output.is_empty() {
        // Fallback if /etc/os-release fails
        let os_name = send_bash_command_with_params("uname", &["-s"]);
        let arch = send_bash_command_with_params("uname", &["-m"]);
        *os = Some(format!("{} {}", os_name, arch));
        return;
    }

    let pretty_name_line = os_release_output
        .lines()
        .find(|line| line.starts_with("PRETTY_NAME="));

    let pretty_name_value = if let Some(line) = pretty_name_line {
        // Try to extract value between quotes
        let between_quotes_regex = Regex::new(r#""([^"]*)""#);
        match between_quotes_regex {
            Ok(re) => {
                if let Some(captures) = re.captures(line) {
                    captures.get(1).map_or("", |m| m.as_str()).to_string()
                } else {
                     // If no quotes, try splitting by =
                     line.split('=').nth(1).unwrap_or("").trim_matches('"').to_string()
                }
            },
            Err(_) => {
                 // Fallback regex extraction
                 line.split('=').nth(1).unwrap_or("").trim_matches('"').to_string()
            }
        }
    } else {
        "".to_string() // Default if PRETTY_NAME not found
    };


    let architechture = send_bash_command_with_params("uname", &["-m"]);

    if pretty_name_value.is_empty() {
         *os = Some(format!("Unknown OS {}", architechture));
    } else {
        *os = Some(format!("{} {}", pretty_name_value, architechture));
    }
}

// get the uptime of the system
// returns something like "26 minutes"
fn get_uptime(uptime: &mut Option<String>) {
    // this will return something like this
    // up 5 min
    let raw_uptime = send_bash_command_with_params("uptime", &["-p"]);

    if !raw_uptime.is_empty() && raw_uptime.starts_with("up ") {
        let raw_uptime_without_up = &raw_uptime[3..]; // Remove "up "
        *uptime = Some(raw_uptime_without_up.trim().to_string());
    } else {
        // Fallback method using /proc/uptime
        if let Ok(uptime_content) = fs::read_to_string("/proc/uptime") {
            let parts: Vec<&str> = uptime_content.split_whitespace().collect();
            if let Some(uptime_secs_str) = parts.first() {
                 if let Ok(uptime_secs_f) = uptime_secs_str.parse::<f64>() {
                     let uptime_secs = uptime_secs_f as u64;
                     let hours = uptime_secs / 3600;
                     let minutes = (uptime_secs % 3600) / 60;
                     if hours > 0 {
                         *uptime = Some(format!("{} hours, {} minutes", hours, minutes));
                     } else {
                         *uptime = Some(format!("{} minutes", minutes));
                     }
                     return;
                 }
            }
        }
        *uptime = Some("Unknown".to_string());
    }
}

/// returns the 'usage' of the systems memory and prints it in MB
/// not exact usage, grabbed from /proc/meminfo
fn get_memory_usage(memory: &mut Option<String>) {
    let proc_file_result = File::open("/proc/meminfo");
     let proc_file = match proc_file_result {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Warning: Could not open /proc/meminfo: {}", e);
            *memory = Some("Unknown Memory".to_string());
            return;
        }
    };
    let reader = BufReader::new(proc_file);

    let mut mem_total: Option<u64> = None; // Use Option<u64> for better handling
    let mut mem_available: Option<u64> = None; // Use Option<u64> for better handling

    for line_result in reader.lines() {
         let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Warning: Error reading line from /proc/meminfo: {}", e);
                continue;
            }
        };

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let key = parts[0].trim_end_matches(':');
            let value_str = parts[1];
            if let Ok(value) = value_str.parse::<u64>() { // Parse as u64
                match key {
                    "MemTotal" => mem_total = Some(value),
                    "MemAvailable" => mem_available = Some(value),
                     _ => {}
                }
            }
        }

        // break out early if we have the info we need
        if mem_total.is_some() && mem_available.is_some() {
            break;
        }
    }

    // Calculate and format memory usage
    match (mem_total, mem_available) {
        (Some(total), Some(available)) => {
            let used = total - available;
            // Convert to MB (1 KB = 1024 bytes)
            let used_mb = used / 1024;
            let total_mb = total / 1024;
            *memory = Some(format!("{} MB / {} MB", used_mb, total_mb));
        },
        _ => {
             *memory = Some("Unknown Memory".to_string());
        }
    }
}

/// gets the user and hostname like 'bill@fedora'
fn get_user_hostname(user_hostname: &mut Option<String>) {
    let username = send_bash_command("whoami");
    let hostname = send_bash_command("hostname");

    if !username.is_empty() && !hostname.is_empty() {
        *user_hostname = Some(format!("{}@{}", username, hostname));
    } else {
        *user_hostname = Some("user@host".to_string()); // Default fallback
    }
}

fn main() {
    let mut sys_info = SystemInfo::new();

    get_cpu_info(&mut sys_info.cpu);
    get_gpu_info(&mut sys_info.gpu);
    get_kernel_info(&mut sys_info.kernel);
    get_os(&mut sys_info.os);
    get_uptime(&mut sys_info.uptime);
    get_memory_usage(&mut sys_info.memory);
    get_user_hostname(&mut sys_info.user_hostname);

    // Print results, handling potential None values more gracefully
    println!("{}", sys_info.user_hostname.as_ref().unwrap_or(&"user@host".to_string()).blue().bold());
    println!("---------------");
    println!(
        "{} {}",
        "OS:".truecolor(56, 83, 120).bold(),
        sys_info.os.as_ref().unwrap_or(&"Unknown OS".to_string())
    );
    println!(
        "{} {}",
        "Kernel:".truecolor(56, 83, 120).bold(),
        sys_info.kernel.as_ref().unwrap_or(&"Unknown Kernel".to_string())
    );
    println!(
        "{} {}",
        "Uptime:".truecolor(56, 83, 120).bold(),
        sys_info.uptime.as_ref().unwrap_or(&"Unknown Uptime".to_string())
    );
    println!(
        "{} {}",
        "CPU:".truecolor(56, 83, 120).bold(),
        sys_info.cpu.as_ref().unwrap_or(&"Unknown CPU".to_string())
    );
    println!(
        "{} {}",
        "GPU:".truecolor(56, 83, 120).bold(),
        sys_info.gpu.as_ref().unwrap_or(&"Unknown GPU".to_string())
    );
    println!(
        "{} {}",
        "Memory:".truecolor(56, 83, 120).bold(),
        sys_info.memory.as_ref().unwrap_or(&"Unknown Memory".to_string())
    );
}