// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

/// This module contains the consent handler trait and a minimal auto-consent implementation that accepts or declines all requests.
pub mod consent_handler;

/// This module contains the installer trait to install the fetched update.
pub mod installer;

/// This module contains the persistent store trait and a minimal file-based implementation.
pub mod persistent_store;

/// This module contains the progress reporter trait to report feedback on the update progress to the user.
pub mod progress_reporter;

/// This module contains the update source trait and a minimal dummy implementation that returns a fixed update info.
pub mod update_source;

/// This module contains the integrity check trait used to check the integrity of the updated system.
pub mod integrity_check;

/// This module contains the update trigger trait used to trigger an update.
pub mod update_trigger;
