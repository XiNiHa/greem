#!/usr/bin/env python3
"""THROWAWAY compiler experiments. Outputs/timings are regenerated on each run."""
from pathlib import Path
import subprocess
import time
root = Path(__file__).resolve().parent
out = root / 'results'
out.mkdir(exist_ok=True)
commands = [
    ('runtime', ['--crate-name', 'probe_runtime', '--crate-type', 'lib', str(root/'src/runtime.rs'), '--out-dir', str(out)]),
    ('types', [str(root/'src/types.rs'), '--extern', f'probe_runtime={out}/libprobe_runtime.rlib', '-o', str(out/'types')]),
    ('missing', ['--cfg', 'missing', str(root/'src/types.rs'), '--extern', f'probe_runtime={out}/libprobe_runtime.rlib', '-o', str(out/'missing')]),
    ('wrong', ['--cfg', 'wrong', str(root/'src/types.rs'), '--extern', f'probe_runtime={out}/libprobe_runtime.rlib', '-o', str(out/'wrong')]),
    ('coherence', ['--cfg', 'sugar', '--cfg', 'delegation', '--crate-type', 'lib', str(root/'src/runtime.rs'), '-o', str(out/'coherence.rlib')]),
]
commands.extend([
    ('runtime_delegation', ['--cfg', 'delegation', '--crate-name', 'probe_runtime', '--crate-type', 'lib', str(root/'src/runtime.rs'), '-o', str(out/'libdelegation.rlib')]),
    ('adapter', [str(root/'src/adapter.rs'), '--extern', f'probe_runtime={out}/libdelegation.rlib', '-o', str(out/'adapter')]),
    ('runtime_sugar_only', ['--cfg', 'sugar', '--crate-name', 'probe_runtime', '--crate-type', 'lib', str(root/'src/runtime.rs'), '-o', str(out/'libsugar.rlib')]),
    ('sugar_with_manual', [str(root/'src/types.rs'), '--extern', f'probe_runtime={out}/libsugar.rlib', '-o', str(out/'sugar_with_manual')]),
])
for name, source, flags in [
    ('reference_original', 'reference.rs', []),
    ('reference_split', 'reference.rs', ['--cfg', 'split']),
    ('scope_loop', 'arena_failures.rs', ['--cfg', 'loop_owned']),
    ('owner_borrow', 'arena_failures.rs', ['--cfg', 'owner_borrow']),
    ('plan_cache', 'arena_failures.rs', ['--cfg', 'plan_cache']),
]:
    commands.append((name, [*flags, str(root/'src'/source), '-o', str(out/name)]))
report = [subprocess.check_output(['rustc', '--version'], text=True).strip()]
for name, args in commands:
    start = time.perf_counter()
    result = subprocess.run(['rustc', '--edition', '2024', *args], text=True, capture_output=True)
    elapsed = time.perf_counter() - start
    (out/f'{name}.txt').write_text(result.stdout + result.stderr)
    report.append(f'{name}: exit={result.returncode}, elapsed={elapsed:.3f}s')
    if name == 'types' and result.returncode == 0:
        report.append(subprocess.check_output([str(out/'types')], text=True).strip())
(out/'summary.txt').write_text('\n'.join(report)+'\n')
print('\n'.join(report))
