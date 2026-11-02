"""Build the distributable EXE without the builder's local paths.

Rust embeds source paths of dependencies (e.g. C:\\Users\\<name>\\.cargo\\registry\\...)
in panic messages. --remap-path-prefix rewrites them. RUSTFLAGS replaces the
rustflags of .cargo/config.toml, so those are read and kept here.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGET = 'x86_64-pc-windows-msvc'


def main():
    config = tomllib.loads((ROOT / '.cargo/config.toml').read_text(encoding='utf-8'))
    flags = list(config['target'][TARGET]['rustflags'])
    cargo_home = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')).resolve()
    rustup_home = Path(os.environ.get('RUSTUP_HOME', Path.home() / '.rustup')).resolve()
    for local, public in ((cargo_home, 'cargo'), (rustup_home, 'rustup'), (ROOT, 'orbit')):
        flags.append(f'--remap-path-prefix={local}={public}')
    env = dict(os.environ, RUSTFLAGS=' '.join(flags))
    cargo = shutil.which('cargo') or str(cargo_home / 'bin' / 'cargo.exe')
    subprocess.run([cargo, 'build', '--locked', '--release', '--no-default-features'],
                   cwd=ROOT, env=env, check=True)
    built = ROOT / 'target/release/orbit-launcher.exe'
    shutil.copyfile(built, ROOT / 'orbit-launcher.exe')
    print(f'Built and copied: {built.relative_to(ROOT)} -> orbit-launcher.exe')


if __name__ == '__main__':
    sys.exit(main())
