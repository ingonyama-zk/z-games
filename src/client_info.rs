// Copyright (C) 2019-2022 Ingonyama
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

use super::{Args, Network};

#[derive(Clone, Debug, serde::Serialize)]
pub struct ClientInfo {
    params: Params,
    hostname: String,
    hardware: Hardware,
    caption: Option<String>,
    hwid: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Params {
    threads: usize,
    threads_in_pool: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Cpu {
    model: String,
    cores: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Gpu {
    model: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Hardware {
    cpu: Vec<Cpu>,
    gpu: Vec<Gpu>,
}

impl ClientInfo {
    pub fn new(args: &Args) -> anyhow::Result<Self> {
        let cpuinfo = procfs::CpuInfo::new()?;
        let cpu_model_name = cpuinfo.fields.get("model name").cloned().unwrap_or_default();
        let cpu_cores = cpuinfo.fields.get("cpu cores").cloned().unwrap_or_default().parse()?;

        let gpus = {
            use rustacuda::prelude::*;
            let mut gpus = vec![];
            if rustacuda::init(CudaFlags::empty()).is_ok() {
                for device in Device::devices()? {
                    let device = device?;
                    let name = device.name()?;
                    gpus.push(Gpu { model: name });
                }
            }
            gpus
        };

        let hostname = uname::uname()?.nodename;

        let hwid = {
            use machineid_rs::{Encryption, HWIDComponent, IdBuilder};
            let mut builder = IdBuilder::new(Encryption::SHA256);
            builder
                .add_component(HWIDComponent::SystemID)
                .add_component(HWIDComponent::CPUCores)
                .add_component(HWIDComponent::CPUID)
                //.add_component(HWIDComponent::DriveSerial) FIXME
                .add_component(HWIDComponent::MacAddress)
                .add_component(HWIDComponent::Username)
                .add_component(HWIDComponent::MachineName);
            builder.build("hwkey")?
        };
        Ok(Self {
            params: Params { threads: args.parallel_num, threads_in_pool: args.threads_num as usize },
            caption: args.caption.clone(),
            hostname,
            hwid,
            hardware: Hardware {
                cpu: vec![Cpu {
                    model: cpu_model_name,
                    cores: cpu_cores,
                }],
                gpu: gpus,
            },
        })
    }
}
