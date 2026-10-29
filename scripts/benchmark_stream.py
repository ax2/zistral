"""Opt-in comparison of two repository SSE decoders; no model/network requests.

Run from a Git checkout with Python3.11+, Cargo and the locked dependencies cached:
  python scripts/benchmark_stream.py --output stream-benchmark.json
The full controlled run deliberately includes the slow former implementation and can
take several minutes. Existing output files are never replaced.
"""
import argparse,json,pathlib,subprocess,sys,tempfile,tomllib

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before',default='v0.29.0',help='Git ref for the old decoder')
    parser.add_argument('--after',default='HEAD',help='Git ref for the comparison decoder')
    parser.add_argument('--output',type=pathlib.Path,help='Save JSON to a new file; default stdout')
    args=parser.parse_args();root=pathlib.Path(__file__).resolve().parent.parent
    if args.output and args.output.exists():parser.error('output already exists; choose a new file')
    def git(*parts):return subprocess.check_output(['git','-C',str(root),*parts])
    old=git('show',args.before+':src/stream.rs');new=git('show',args.after+':src/stream.rs')
    packages=tomllib.loads(git('show',args.after+':Cargo.lock').decode())['package']
    versions={p['name']:p['version'] for p in packages if p['name'] in {'anyhow','serde_json'}}
    manifest='''[package]
name="zistral-stream-bench"
version="0.1.0"
edition="2024"
[[bin]]
name="zistral-stream-bench"
path="main.rs"
[dependencies]
anyhow="=%s"
serde_json={version="=%s",features=["raw_value","unbounded_depth"]}
[profile.release]
opt-level="s"
lto=true
codegen-units=1
strip=true
'''%(versions['anyhow'],versions['serde_json'])
    temp_root=pathlib.Path(tempfile.gettempdir()).resolve()
    with tempfile.TemporaryDirectory(prefix='zistral-stream-bench-',dir=temp_root) as temporary:
        work=pathlib.Path(temporary).resolve()
        assert work.parent==temp_root and work.name.startswith('zistral-stream-bench-')
        (work/'Cargo.toml').write_text(manifest,encoding='utf-8');(work/'Cargo.lock').write_bytes(git('show',args.after+':Cargo.lock'))
        (work/'before.rs').write_bytes(old);(work/'after.rs').write_bytes(new)
        (work/'main.rs').write_bytes((root/'benchmarks/stream-decoder/main.rs').read_bytes())
        # Offline is deliberate: this measurement never installs or fetches dependencies.
        subprocess.run(['cargo','build','--release','--offline','--manifest-path',str(work/'Cargo.toml')],check=True,stdout=sys.stderr)
        metadata=json.loads(subprocess.check_output(['cargo','metadata','--format-version','1','--offline','--no-deps','--manifest-path',str(work/'Cargo.toml')]))
        binary=pathlib.Path(metadata['target_directory'])/'release'/('zistral-stream-bench.exe' if sys.platform=='win32' else 'zistral-stream-bench')
        result=json.loads(subprocess.check_output([str(binary)]))
    result['refs']={'before':git('rev-parse',args.before).decode().strip(),'after':git('rev-parse',args.after).decode().strip()}
    result['dependency_versions']=versions
    text=json.dumps(result,ensure_ascii=False,indent=2)+'\n'
    if args.output:
        with args.output.open('x',encoding='utf-8') as output:output.write(text)
    else:sys.stdout.write(text)

if __name__=='__main__':main()
