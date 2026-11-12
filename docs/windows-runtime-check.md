# Windows standalone payload check (v0.34.0)

The Windows x64 build already enables `+crt-static` in
[Cargo configuration](../.cargo/config.toml). The published v0.33.0 executable and
a release candidate were independently inspected: both import only the eight
reviewed Windows modules below. No missing Visual C++ redistributable was found;
this change adds a packaging regression gate, not a runtime or installer payload.

```text
advapi32.dll
api-ms-win-core-synch-l1-2-0.dll
bcrypt.dll
bcryptprimitives.dll
kernel32.dll
ntdll.dll
userenv.dll
ws2_32.dll
```

Before creating a Windows archive or installer, `scripts/package.py` runs
[the dependency checker](../scripts/windows_runtime.py) on the exact executable.
It invokes Microsoft's DUMPBIN with `/DEPENDENTS`, checks a reviewed import set,
and reports the inspected binary's SHA256. Unknown imports, unavailable tooling,
tool failure or an unrecognizable/empty import list fail before packaging.
This catches accidentally dynamic C/C++ runtimes and added helper/OpenSSL/language
DLLs even when the build machine happens to have those libraries installed.
New legitimate system imports need explicit review of the allowlist.

The checker uses the build host's `dumpbin.exe` on PATH, or locates it through
Visual Studio's `vswhere`. `vswhere -utf8` preserves Unicode installation paths.
Developer inspection can specify the tool explicitly:

```powershell
python scripts/windows_runtime.py --binary path\to\zi.exe --dumpbin path\to\dumpbin.exe
```

For packaging, use a C++ build host where DUMPBIN is on PATH or Visual Studio is
discoverable. Python, vswhere, DUMPBIN and the rule tests are build/test tools;
they are not copied into the archive or installed with zi. Both candidate and
actual Release packaging use the same gate. Changes to Cargo's Windows linker
configuration or the checker also trigger candidate installer validation.

Six portable rule tests run in normal CI and Release preparation. Actual local
MSVC `/MD` and `/MT` control programs establish that a dynamic-runtime executable
is rejected and a static-runtime one is accepted. Native Windows CI additionally
checks the real release-mode payload before packaging, then retains its existing
installation, pipe, configuration, PATH-ownership and uninstall checks.

This is a direct PE import check. It does not prove compatibility with every
Windows edition, recursively inspect system DLLs, observe arbitrary dynamic
loading, or replace clean-machine acceptance and actual installed behavior.
It does not inspect the installer's own imports, bundle a model, or establish
model-service access. Supported OS components still come from Windows.

## Minimum Windows version

Unreleased `main` explicitly sets `MinVersion=10.0` in the Windows installer,
matching the Rust MSVC target's Windows 10+ / Windows Server 2016+ requirement.
The check applies to the installer; portable users need the same supported OS.
Leaving this directive unspecified uses Inno Setup's Windows 7 SP1 default,
which must not be mistaken for the Rust executable's supported platform range.

The existing native installer checks compile and exercise the package on the
current Windows CI host. They do not simulate an older Windows version or claim
an acceptance test on every supported edition/build. This change updates installer
metadata and packaged README text; application logic and model settings stay
unchanged, with no added runtime payload.

References: [Rust MSVC target requirements](https://github.com/rust-lang/rust/blob/main/src/doc/rustc/src/platform-support/windows-msvc.md),
[Inno Setup MinVersion](https://jrsoftware.org/ishelp/topic_setup_minversion.htm).

Primary references: [Microsoft DUMPBIN /DEPENDENTS](https://learn.microsoft.com/en-us/cpp/build/reference/dependents?view=msvc-170),
[Rust C runtime linkage](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes),
[Microsoft vswhere output encoding](https://github.com/microsoft/vswhere/wiki/Encoding).
