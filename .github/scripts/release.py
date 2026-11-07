"""Local checks and packaging shared by the Windows release workflow."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def version():
    return tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']


def validate():
    expected = f'v{version()}'
    if os.environ.get('GITHUB_REPOSITORY') != 'azukihoney/orbit-launcher':
        raise ValueError('Only the public release repository may request signing')
    event = os.environ.get('GITHUB_EVENT_NAME')
    if event not in ('push', 'workflow_dispatch'):
        raise ValueError('Only tag pushes and manual runs may request signing')
    if os.environ.get('RELEASE_TAG') != expected:
        raise ValueError(f'Release tag must match Cargo.toml: {expected}')
    if event == 'push' and os.environ.get('GITHUB_REF') != f'refs/tags/{expected}':
        raise ValueError('Push must be for the release tag')
    tag_commit = subprocess.check_output(
        ['git', 'rev-parse', f'refs/tags/{expected}^{{commit}}'], cwd=ROOT, text=True
    ).strip()
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if head != tag_commit:
        raise ValueError('Checkout does not match the release tag')
    print(f'Validated {expected}: {head}')


def check_paths(binary):
    homes = [Path.home(), ROOT,
             Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')).resolve(),
             Path(os.environ.get('RUSTUP_HOME', Path.home() / '.rustup')).resolve()]
    markers = {'runneradmin'}
    for path in homes:
        markers.update((str(path).replace('\\', '/'), str(path).replace('/', '\\')))
    payload = Path(binary).read_bytes().lower()
    for marker in markers:
        for encoding in ('utf-8', 'utf-16le'):
            if marker.lower().encode(encoding) in payload:
                raise ValueError('Refuse binary containing a builder path or runner name')
    print(f'Additional path check passed: {binary}')


def package():
    signed = ROOT / 'signed/orbit-launcher.exe'
    check_paths(signed)
    digest = hashlib.sha256(signed.read_bytes()).digest()
    for target in (ROOT / 'orbit-launcher.exe', ROOT / 'target/release/orbit-launcher.exe'):
        shutil.copyfile(signed, target)
        if hashlib.sha256(target.read_bytes()).digest() != digest:
            raise ValueError('Signed binary copy hash mismatch')
    subprocess.run([sys.executable, str(ROOT / 'tools/package.py')], cwd=ROOT, check=True)
    archive = ROOT / f'dist/orbit-launcher-{version()}-windows-x64.zip'
    with zipfile.ZipFile(archive) as zipped:
        if hashlib.sha256(zipped.read('orbit-launcher.exe')).digest() != digest:
            raise ValueError('ZIP does not contain the verified signed binary')
    zip_digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if (ROOT / 'dist/SHA256SUMS.txt').read_text(encoding='ascii') != f'{zip_digest}  {archive.name}\n':
        raise ValueError('ZIP checksum file mismatch')
    summary = f'EXE SHA-256: `{digest.hex()}`\n\nZIP SHA-256: `{zip_digest}`\n'
    print(summary)
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a', encoding='utf-8') as stream:
            stream.write(summary)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('validate', 'paths', 'package'))
    parser.add_argument('--binary', type=Path, default=ROOT / 'orbit-launcher.exe')
    args = parser.parse_args()
    if args.command == 'validate':
        validate()
    elif args.command == 'paths':
        check_paths(args.binary)
    else:
        package()
