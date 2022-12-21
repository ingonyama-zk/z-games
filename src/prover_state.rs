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

use tokio_rayon::AsyncRayonHandle;

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32},
        Arc,
        RwLock,
    },
    time::Instant,
};

#[derive(Clone)]
pub(crate) struct ProverState {
    pub(crate) task_id: String,
    pub(crate) pools: Arc<RwLock<Vec<AsyncRayonHandle<()>>>>,
    pub(crate) terminator: Arc<AtomicBool>,
    pub(crate) proves_count: Arc<AtomicU32>,
    pub(crate) proves_start: Arc<RwLock<Instant>>,
}

impl ProverState {
    pub(crate) fn new(task_id: String) -> Self {
        Self {
            task_id,
            pools: Default::default(),
            terminator: Arc::new(AtomicBool::new(false)),
            proves_count: Arc::new(AtomicU32::new(0)),
            proves_start: Arc::new(RwLock::new(Instant::now())),
        }
    }
}
