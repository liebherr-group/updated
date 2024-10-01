// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use anyhow::Result;
use vergen::EmitBuilder;

pub fn main() -> Result<()> {
    EmitBuilder::builder()
        .build_date()
        .git_describe(true, false, None)
        .cargo_target_triple()
        .emit()?;
    Ok(())
}
