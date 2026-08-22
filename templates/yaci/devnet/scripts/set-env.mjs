// Publishes (or clears) the Yaci devnet connection details in the project's
// shared ../.env, so off-chain/devnet consumers find them at the standard
// keys defined by the cardano-init interface contract.
//
//   node scripts/set-env.mjs            # write the devnet connection details
//   node scripts/set-env.mjs --clear    # blank them out again (used by `clean`)
//
// The write is idempotent: existing keys are replaced in place (no duplicate
// lines accumulate across repeated `just dev` runs); every other line in the
// .env — CARDANO_NETWORK, comments, blanks — is preserved untouched.
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  closeSync,
  ftruncateSync,
  linkSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  rmdirSync,
  unlinkSync,
  writeSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

// This script lives at test/scripts/; the project root is two levels up.
const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const envPath = resolve(projectRoot, ".env");
const lockPath = `${envPath}.lock`;
const lockOwnerPath = `${lockPath}/owner`;
const ownProcessStart = processStart(process.pid);
if (ownProcessStart === null) {
  throw new Error("Unable to identify lock owner process");
}
const lockOwnerRecord = `${process.pid}\n${ownProcessStart}\n`;
const lockOwnerDraft = `${lockPath}.owner.${process.pid}.${randomUUID()}`;

const clear = process.argv.includes("--clear");

// Standard local Yaci DevKit endpoints (see ./README.md and the devkit docs).
// INDEXER_URL / INDEXER_PORT are the contract's connection keys; YACI_ADMIN_URL
// is an extra key Yaci adds for its faucet/topup admin API.
const SET = {
  INDEXER_URL: "http://localhost:8080/api/v1/",
  INDEXER_PORT: "8080",
  YACI_ADMIN_URL: "http://localhost:10000",
};

// On clear: blank the contract keys; drop the Yaci-specific admin key entirely.
const updates = clear
  ? { INDEXER_URL: "", INDEXER_PORT: "" }
  : SET;
const removeKeys = clear ? new Set(["YACI_ADMIN_URL"]) : new Set();

function hasCode(error, code) {
  return error !== null && typeof error === "object" && error.code === code;
}

function openEnv(path) {
  try {
    return openSync(path, "r+");
  } catch (error) {
    if (!hasCode(error, "ENOENT")) throw error;
    try {
      return openSync(path, "wx+", 0o600);
    } catch (createError) {
      if (!hasCode(createError, "EEXIST")) throw createError;
      return openSync(path, "r+");
    }
  }
}

function writeAll(fd, text, path) {
  const buffer = Buffer.from(text, "utf-8");
  let offset = 0;
  while (offset < buffer.length) {
    const written = writeSync(fd, buffer, offset, buffer.length - offset, offset);
    if (written === 0) throw new Error(`Unable to make progress writing ${path}`);
    offset += written;
  }
}

function unlinkIfPresent(path) {
  try {
    unlinkSync(path);
  } catch (error) {
    if (!hasCode(error, "ENOENT")) throw error;
  }
}

function processStart(pid) {
  const result = spawnSync("ps", ["-o", "lstart=", "-p", String(pid)], {
    encoding: "utf-8",
    stdio: ["ignore", "pipe", "ignore"],
  });
  if (result.status !== 0) return null;
  const start = result.stdout.trim().replace(/\s+/g, " ");
  return start.length > 0 ? start : null;
}

function lockOwnerIsStale(record) {
  const lines = record.trimEnd().split("\n");
  if (lines.length !== 2 || !/^[1-9][0-9]*$/.test(lines[0])) return true;

  const pid = Number(lines[0]);
  if (!Number.isSafeInteger(pid) || lines[1].length === 0) return true;
  return processStart(pid) !== lines[1];
}

function reclaimStaleLock(path) {
  let record;
  try {
    record = readFileSync(lockOwnerPath, "utf-8");
  } catch (error) {
    if (!hasCode(error, "ENOENT")) throw error;
    try {
      rmdirSync(path);
      return true;
    } catch (removeError) {
      if (hasCode(removeError, "ENOENT")) return true;
      if (hasCode(removeError, "ENOTEMPTY") || hasCode(removeError, "EEXIST")) {
        return false;
      }
      throw removeError;
    }
  }

  if (!lockOwnerIsStale(record)) return false;

  const staleOwner = `${path}.stale.${process.pid}.${randomUUID()}`;
  try {
    renameSync(lockOwnerPath, staleOwner);
  } catch (error) {
    if (hasCode(error, "ENOENT")) return false;
    throw error;
  }

  try {
    try {
      rmdirSync(path);
      return true;
    } catch (error) {
      if (hasCode(error, "ENOENT")) return true;
      if (hasCode(error, "ENOTEMPTY") || hasCode(error, "EEXIST")) return false;
      throw error;
    }
  } finally {
    unlinkIfPresent(staleOwner);
  }
}

async function acquireLock(path) {
  const retryMs = 100;
  const attempts = 300;
  const ownerFd = openSync(lockOwnerDraft, "wx", 0o600);
  try {
    writeAll(ownerFd, lockOwnerRecord, lockOwnerDraft);
  } finally {
    closeSync(ownerFd);
  }

  for (let attempt = 0; attempt < attempts; attempt++) {
    try {
      mkdirSync(path, { mode: 0o700 });
      try {
        linkSync(lockOwnerDraft, lockOwnerPath);
        unlinkIfPresent(lockOwnerDraft);
        return;
      } catch (error) {
        if (!hasCode(error, "ENOENT") && !hasCode(error, "EEXIST")) throw error;
      }
    } catch (error) {
      if (!hasCode(error, "EEXIST")) throw error;
      if (reclaimStaleLock(path)) continue;
      if (attempt === attempts - 1) {
        throw new Error(`Timed out waiting for ${path}`);
      }
      await delay(retryMs);
    }
  }
  throw new Error(`Unable to acquire ${path}`);
}

try {
  await acquireLock(lockPath);
  try {
    const envFd = openEnv(envPath);
    try {
      const original = readFileSync(envFd, "utf-8");
      const lines = original.length > 0 ? original.split("\n") : [];

      const pending = new Set(Object.keys(updates));
      const out = [];
      for (const line of lines) {
        const match = /^([A-Za-z_][A-Za-z0-9_]*)=/.exec(line);
        const key = match?.[1];
        if (key && removeKeys.has(key)) continue; // drop this line
        if (key && pending.has(key)) {
          out.push(`${key}=${updates[key]}`);
          pending.delete(key);
        } else {
          out.push(line);
        }
      }
      // Append any keys that weren't already present.
      for (const key of pending) out.push(`${key}=${updates[key]}`);

      // Normalize to a single trailing newline (LF).
      let text = out.join("\n").replace(/\n+$/, "");
      if (text.length > 0) text += "\n";
      ftruncateSync(envFd, 0);
      writeAll(envFd, text, envPath);
    } finally {
      closeSync(envFd);
    }
  } finally {
    let ownsLock = false;
    try {
      ownsLock = readFileSync(lockOwnerPath, "utf-8") === lockOwnerRecord;
    } catch (error) {
      if (!hasCode(error, "ENOENT")) throw error;
    }
    if (ownsLock) {
      unlinkSync(lockOwnerPath);
      try {
        rmdirSync(lockPath);
      } catch (error) {
        if (!hasCode(error, "ENOENT")) throw error;
      }
    }
  }
} finally {
  unlinkIfPresent(lockOwnerDraft);
}

if (clear) {
  console.log(`Cleared Yaci connection details from ${envPath}`);
} else {
  console.log(`Wrote Yaci connection details to ${envPath}:`);
  for (const [k, v] of Object.entries(SET)) console.log(`  ${k}=${v}`);
}
