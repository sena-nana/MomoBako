// 覆盖 .momoplug 双清单、相对路径与 artifact 完整性规则。
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";
import { stringify } from "smol-toml";
import {
  assertDeclaredCompanionArtifacts,
  refreshPluginArtifactHashes,
  validatePluginPackage,
  writePluginPackageEnvelope,
} from "./plugin-package-manifest.ts";

const temporaryDirectories: string[] = [];

function fixtureDir(): string {
  const path = mkdtempSync(join(tmpdir(), "momobako-plugin-package-"));
  temporaryDirectories.push(path);
  return path;
}

function sha256(value: string): string {
  return `sha256:${createHash("sha256").update(value).digest("hex")}`;
}

function writeMomoManifest(
  directory: string,
  overrides: Record<string, unknown> = {},
): void {
  writeFileSync(
    join(directory, "manifest.json"),
    JSON.stringify({
      pluginId: "momobako.test",
      version: "1.2.3",
      runtime: "native-dylib",
      entry: { backend: { library: "test" } },
      ...overrides,
    }),
  );
}

function writePluginToml(
  directory: string,
  overrides: Record<string, unknown> = {},
): void {
  const manifest = {
    plugin_id: "momobako.test",
    version: "1.2.3",
    artifact: {
      artifact_type: "abi",
      path: "plugin.dll",
      sha256: sha256("plugin"),
      companion_artifacts: [{
        path: "bin/helper.exe",
        sha256: sha256("helper"),
        executable: true,
        role: "office-convert-helper",
      }],
    },
    provides: {
      handler_bindings: [{
        binding_id: "binding:momobako.test:momobako.test.ping",
        plugin_id: "momobako.test",
        protocol_id: "momobako.test.ping",
        target_runner_hint: "momobako.test.runner",
      }],
    },
    ...overrides,
  };
  writeFileSync(join(directory, "plugin.toml"), stringify(manifest));
}

function writeValidExecutableFixture(): string {
  const directory = fixtureDir();
  mkdirSync(join(directory, "bin"));
  writeFileSync(join(directory, "plugin.dll"), "plugin");
  writeFileSync(join(directory, "bin", "helper.exe"), "helper");
  writeMomoManifest(directory);
  writePluginToml(directory);
  writePluginPackageEnvelope(directory, "x86_64-pc-windows-msvc");
  return directory;
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("manifest-only and frontend packages do not require plugin.toml", () => {
  for (const manifest of [
    { runtime: "manifest-only", entry: {} },
    { runtime: "vue-module", entry: { frontend: { module: "dist/index.js" } } },
  ]) {
    const directory = fixtureDir();
    writeMomoManifest(directory, manifest);
    if (manifest.runtime === "vue-module") {
      mkdirSync(join(directory, "dist"));
      writeFileSync(join(directory, "dist", "index.js"), "export default {};");
    }
    writePluginPackageEnvelope(directory, "x86_64-pc-windows-msvc");
    assert.equal(validatePluginPackage(directory).pluginToml, undefined);
  }
});

test("executable backend packages require plugin.toml", () => {
  const directory = fixtureDir();
  writeMomoManifest(directory);
  writePluginPackageEnvelope(directory, "x86_64-pc-windows-msvc");
  assert.throws(
    () => validatePluginPackage(directory),
    /executable backend plugin requires plugin\.toml/,
  );
});

test("validates matching dual manifests and companion artifacts", () => {
  const directory = writeValidExecutableFixture();
  const result = validatePluginPackage(directory);
  assert.equal(result.manifest.pluginId, "momobako.test");
  assert.equal(result.pluginToml?.artifact.companion_artifacts?.[0]?.role, "office-convert-helper");
});

test("rejects mismatched plugin ids and versions", () => {
  const idDirectory = writeValidExecutableFixture();
  writePluginToml(idDirectory, { plugin_id: "momobako.other" });
  assert.throws(() => validatePluginPackage(idDirectory), /plugin id mismatch/);

  const versionDirectory = writeValidExecutableFixture();
  writePluginToml(versionDirectory, { version: "9.9.9" });
  assert.throws(() => validatePluginPackage(versionDirectory), /plugin version mismatch/);
});

test("requires plugin-scoped unique handler bindings", () => {
  const legacyBindingDirectory = writeValidExecutableFixture();
  writePluginToml(legacyBindingDirectory, {
    provides: {
      handler_bindings: [{
        binding_id: "binding:momobako.test.ping",
        plugin_id: "momobako.test",
        protocol_id: "momobako.test.ping",
      }],
    },
  });
  assert.throws(
    () => validatePluginPackage(legacyBindingDirectory),
    /handler binding id must be plugin-scoped/,
  );

  const duplicateBindingDirectory = writeValidExecutableFixture();
  const binding = {
    binding_id: "binding:momobako.test:momobako.test.ping",
    plugin_id: "momobako.test",
    protocol_id: "momobako.test.ping",
  };
  writePluginToml(duplicateBindingDirectory, {
    provides: { handler_bindings: [binding, binding] },
  });
  assert.throws(
    () => validatePluginPackage(duplicateBindingDirectory),
    /duplicate handler binding id/,
  );
});

test("rejects invalid artifact paths", () => {
  const invalidPaths = [
    "../plugin.dll",
    "/plugin.dll",
    "C:/plugin.dll",
    "bin\\plugin.dll",
    "./plugin.dll",
    "bin//plugin.dll",
  ];
  for (const path of invalidPaths) {
    const directory = writeValidExecutableFixture();
    writePluginToml(directory, {
      artifact: {
        artifact_type: "abi",
        path,
        sha256: sha256("plugin"),
        companion_artifacts: [],
      },
    });
    assert.throws(() => validatePluginPackage(directory), /path/);
  }
});

test("rejects missing files and non-canonical or mismatched hashes", () => {
  const missingDirectory = writeValidExecutableFixture();
  writePluginToml(missingDirectory, {
    artifact: {
      artifact_type: "abi",
      path: "missing.dll",
      sha256: sha256("plugin"),
      companion_artifacts: [],
    },
  });
  assert.throws(() => validatePluginPackage(missingDirectory), /file is missing/);

  const invalidHashDirectory = writeValidExecutableFixture();
  writePluginToml(invalidHashDirectory, {
    artifact: {
      artifact_type: "abi",
      path: "plugin.dll",
      sha256: "sha256:ABC",
      companion_artifacts: [],
    },
  });
  assert.throws(() => validatePluginPackage(invalidHashDirectory), /64 lowercase hex/);

  const mismatchDirectory = writeValidExecutableFixture();
  writeFileSync(join(mismatchDirectory, "plugin.dll"), "changed");
  assert.throws(() => validatePluginPackage(mismatchDirectory), /sha256 mismatch/);
});

test("refreshes only declared artifact hashes before package validation", () => {
  const directory = writeValidExecutableFixture();
  writeFileSync(join(directory, "plugin.dll"), "rebuilt-plugin");
  writeFileSync(join(directory, "bin", "helper.exe"), "rebuilt-helper");

  refreshPluginArtifactHashes(directory);
  writePluginPackageEnvelope(directory, "x86_64-pc-windows-msvc");

  const result = validatePluginPackage(directory);
  assert.equal(result.pluginToml?.artifact.sha256, sha256("rebuilt-plugin"));
  assert.equal(
    result.pluginToml?.artifact.companion_artifacts?.[0]?.sha256,
    sha256("rebuilt-helper"),
  );
});

test("requires companion build outputs to be declared by plugin.toml", () => {
  const directory = writeValidExecutableFixture();
  assert.doesNotThrow(() => assertDeclaredCompanionArtifacts(directory, ["bin/helper.exe"]));
  assert.throws(
    () => assertDeclaredCompanionArtifacts(directory, ["bin/undeclared.exe"]),
    /not declared in plugin\.toml/,
  );
  assert.throws(
    () => assertDeclaredCompanionArtifacts(directory, ["bin/helper.exe", "bin/helper.exe"]),
    /duplicate companion build artifact path/,
  );
});
