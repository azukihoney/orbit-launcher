"""Package only the explicit public file allowlist, never local configuration or research."""
import hashlib
from pathlib import Path
import tomllib
import zipfile

ROOT=Path(__file__).resolve().parents[1]


def verify_distribution_binary(binary):
    payload=Path(binary).read_bytes()
    for marker in ('Orbit review recycle substitute','Review-only synthetic move loop','ORBIT_STAGE2_PROBE','ORBIT_STAGE2_PAUSE',
                   'ORBIT_STAGE3_PROBE','ORBIT_STAGE3_FIXTURE'):
        assert marker.encode() not in payload and marker.encode('utf-16le') not in payload,'Refuse to distribute a review-test build'
    # Build with tools/build_release.py so that local paths (user names) are not embedded.
    for local in ('\\Users\\', str(Path.home()), str(ROOT)):
        for encoding in ('utf-8', 'utf-16le'):
            assert local.encode(encoding).lower() not in payload.lower(),f'Refuse to distribute a build with a local path: {local}'


def main():
    version=tomllib.loads((ROOT/'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    output=ROOT/'dist';output.mkdir(exist_ok=True)
    destination=output/f'orbit-launcher-{version}-windows-x64.zip'
    scripts=[ROOT/'start.cmd']
    files=[(ROOT/'orbit-launcher.exe','orbit-launcher.exe'),
           *((p,p.name) for p in scripts),
           (ROOT/'resources/README-PORTABLE.txt','README.txt'),
           (ROOT/'LICENSE','LICENSE.txt'),
           (ROOT/'THIRD_PARTY_NOTICES.md','THIRD_PARTY_NOTICES.md'),
           (ROOT/'LICENSES.md','LICENSES.md')]
    files.extend((p,p.relative_to(ROOT).as_posix()) for p in sorted((ROOT/'licenses').rglob('*')) if p.is_file())
    assert files and all(p.is_file() for p,_ in files)
    build=ROOT/'target/release/orbit-launcher.exe'
    for binary in (build, ROOT/'orbit-launcher.exe'):
        verify_distribution_binary(binary)
    # cmd.exe reads scripts in the OEM code page, so only ASCII survives on every PC.
    # Bare LF can make cmd.exe misread labels and blocks, so require CRLF.
    for script in scripts:
        raw=script.read_bytes()
        assert raw.isascii(),f'{script.name} must be ASCII'
        assert raw.count(b'\n')==raw.count(b'\r\n')==raw.count(b'\r'),f'{script.name} must use CRLF'
    assert hashlib.sha256(build.read_bytes()).digest()==hashlib.sha256((ROOT/'orbit-launcher.exe').read_bytes()).digest()
    with zipfile.ZipFile(destination,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as archive:
        for source,name in files:archive.write(source,name)
    with zipfile.ZipFile(destination) as archive:
        assert len(archive.infolist())==len(files)
        for source,name in files:
            assert hashlib.sha256(archive.read(name)).digest()==hashlib.sha256(source.read_bytes()).digest()
        assert not any(n.lower().endswith(('.json','.dat','.ini','.csv','.html')) for n in archive.namelist())
    digest=hashlib.sha256(destination.read_bytes()).hexdigest()
    (output/'SHA256SUMS.txt').write_text(f'{digest}  {destination.name}\n',encoding='ascii')
    print(f'Portable ZIP verified: {destination.name}; files={len(files)}; bytes={destination.stat().st_size}')


if __name__=='__main__':
    main()
