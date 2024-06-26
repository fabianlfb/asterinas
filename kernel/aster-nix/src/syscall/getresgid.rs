// SPDX-License-Identifier: MPL-2.0

use super::SyscallReturn;
use crate::{prelude::*, process::credentials, util::write_val_to_user};

pub fn sys_getresgid(rgid_ptr: Vaddr, egid_ptr: Vaddr, sgid_ptr: Vaddr) -> Result<SyscallReturn> {
    debug!("rgid_ptr = 0x{rgid_ptr:x}, egid_ptr = 0x{egid_ptr:x}, sgid_ptr = 0x{sgid_ptr:x}");

    let credentials = credentials();

    let rgid = credentials.rgid();
    write_val_to_user(rgid_ptr, &rgid)?;

    let egid = credentials.egid();
    write_val_to_user(egid_ptr, &egid)?;

    let sgid = credentials.sgid();
    write_val_to_user(sgid_ptr, &sgid)?;

    Ok(SyscallReturn::Return(0))
}
