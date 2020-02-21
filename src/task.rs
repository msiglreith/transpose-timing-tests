use std::fmt;
use std::fs::OpenOptions;
use std::fmt::Write;

#[allow(dead_code)]
#[derive(Clone)]
pub enum KernelType {
    Threadgroup,
    Ballot,
    Shuffle,
}

impl fmt::Display for KernelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelType::Threadgroup => write!(f, "{}", "threadgroup"),
            KernelType::Ballot => write!(f, "{}", "ballot"),
            KernelType::Shuffle => write!(f, "{}", "shuffle"),
        }
    }
}

#[derive(Clone)]
pub enum BackendVariant {
    #[cfg(feature = "vk")]
    Vk,
    #[cfg(feature = "dx12")]
    Dx12,
}

impl fmt::Display for BackendVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "vk")]
            BackendVariant::Vk => write!(f, "{}", "vk"),
            #[cfg(feature = "dx12")]
            BackendVariant::Dx12 => write!(f, "{}", "dx12"),
        }
    }
}

#[derive(Clone)]
pub struct Task {
    pub name: String,
    pub device_name: String,
    pub num_bms: u32,
    pub workgroup_size: [u32; 2],
    pub num_execs_gpu: u32,
    pub num_execs_cpu: u32,
    pub kernel_type: KernelType,
    pub backend: BackendVariant,
    pub instant_times: Vec<f64>,
    pub timestamp_query_times: Vec<f64>,
}

impl Task {
    pub fn timestamp_time_stats(&self) -> (usize, f64, f64) {
        let avg_time = self.timestamp_query_times.iter().sum::<f64>()
            / (self.timestamp_query_times.len() as f64);
        let std_time = (self
            .timestamp_query_times
            .iter()
            .map(|t| (t - avg_time).powf(2.0))
            .sum::<f64>()
            / (self.timestamp_query_times.len() as f64))
            .powf(0.5);
        (self.timestamp_query_times.len(), avg_time, std_time)
    }

    pub fn instant_time_stats(&self) -> (usize, f64, f64) {
        let avg_time = self.instant_times.iter().sum::<f64>() / (self.instant_times.len() as f64);
        let std_time = (self
            .instant_times
            .iter()
            .map(|t| (t - avg_time).powf(2.0))
            .sum::<f64>()
            / (self.instant_times.len() as f64))
            .powf(0.5);
        (self.instant_times.len(), avg_time, std_time)
    }

    pub fn compile_kernel(&self) -> std::fs::File {
        let kernel_name = self.kernel_name();
        let kernel_fp = format!("kernels/{}.spv", &kernel_name);
        match OpenOptions::new().read(true).open(&kernel_fp) {
            Ok(f) => {
                println!("{} kernel already compiled...", &kernel_name);
                f
            }
            Err(_) => {
                println!("compiling kernel {}...", &kernel_name);
                let glsl = self.materialize_kernel();
                let mut compiler = shaderc::Compiler::new().unwrap();
                let mut options = shaderc::CompileOptions::new().unwrap();
                options.set_target_env(
                    shaderc::TargetEnv::Vulkan,
                    ((1 as u32) << 22) | ((1 as u32) << 12),
                );
                let artifact = compiler
                    .compile_into_spirv(
                        &glsl,
                        shaderc::ShaderKind::Compute,
                        &format!("{}.glsl", kernel_name),
                        "main",
                        Some(&options),
                    )
                    .unwrap();
                let mut compiled_kernel = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&kernel_fp)
                    .unwrap();
                use std::io::Write;
                compiled_kernel.write_all(artifact.as_binary_u8()).unwrap();

                compiled_kernel
            }
        }
    }

    pub fn materialize_kernel(&self) -> String {
        let tp = format!("kernels/transpose-{}-template.comp", self.kernel_type);
        let mut kernel =
            std::fs::read_to_string(&tp).expect(&format!("could not kernel template at path: {}", &tp));

        match self.kernel_type {
            KernelType::Threadgroup => {
                kernel = kernel.replace("~WG_SIZE~", &format!("{}", self.workgroup_size[0]));
                kernel = kernel.replace("~NUM_EXECS~", &format!("{}", self.num_execs_gpu));
            }
            _ => {
                if self.workgroup_size[1] > 1 {
                    panic!("does not make sense to have Y-dimension in workgroup size for subgroup kernels");
                }
                kernel = kernel.replace("~WG_SIZE~", &format!("{}", self.workgroup_size[0]));
                kernel = kernel.replace("~NUM_EXECS~", &format!("{}", self.num_execs_gpu));
            }
        }

        #[cfg(debug_assertions)]
            std::fs::write(format!("kernels/{}.comp", &self.kernel_name()), &kernel).unwrap();

        kernel
    }

    pub fn kernel_name(&self) -> String {
        format!(
            "transpose-{}-WGS=({},{})",
            self.kernel_type, self.workgroup_size[0], self.workgroup_size[1]
        )
    }

    pub fn delete_compiled_kernel(&self) {
        let kp = format!("kernels/{}.spv", self.kernel_name());
        match std::fs::read(&kp) {
            Ok(_) => {
                std::fs::remove_file(&kp).expect("could not delete compiled kernel");
            },
            _ => {}
        };
    }
}

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (ts_n, ts_avg, ts_std) = self.timestamp_time_stats();
        let (its_n, its_avg, its_std) = self.instant_time_stats();
        let mut s = String::new();
        write!(s, "\ntask name:{}\n", self.name).unwrap();
        write!(s, "device: {}\n", self.device_name).unwrap();
        write!(
            s,
            "num BMs: {}, TG size: {}\n",
            self.num_bms,
            self.workgroup_size[0] * self.workgroup_size[1]
        )
            .unwrap();
        write!(
            s,
            "CPU loops: {}, GPU loops: {}\n",
            self.num_execs_cpu, self.num_execs_gpu
        )
            .unwrap();
        write!(
            s,
            "timestamp stats (N = {}): {:.2} +/- {:.2} ms\n",
            ts_n, ts_avg, ts_std
        )
            .unwrap();
        write!(
            s,
            "instant stats (N = {}): {:.2} +/- {:.2} ms",
            its_n, its_avg, its_std
        )
            .unwrap();
        write!(f, "{}", s)
    }
}