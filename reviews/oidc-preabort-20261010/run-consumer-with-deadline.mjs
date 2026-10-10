import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';
const started = performance.now();
const child = spawn(process.execPath, ['--unhandled-rejections=strict', process.argv[2], process.argv[3]], {
  stdio: 'inherit',
});
let deadlineHit = false;
const timer = setTimeout(() => {
  deadlineHit = true;
  child.kill('SIGKILL');
}, 5000);
child.on('error', (error) => {
  clearTimeout(timer);
  console.error(error);
  process.exitCode = 125;
});
child.on('close', (code, signal) => {
  clearTimeout(timer);
  console.log(JSON.stringify({ exitCode: code, signal, deadlineHit, elapsedMs: Math.round(performance.now() - started), childClosed: true }));
  process.exitCode = deadlineHit ? 124 : code ?? 125;
});
