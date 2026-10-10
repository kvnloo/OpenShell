import { spawn } from 'node:child_process';

const deadlineMs = Number(process.argv[2]);
const command = process.argv[3];
const args = process.argv.slice(4);
if (!Number.isInteger(deadlineMs) || deadlineMs < 1 || !command) throw new Error('invalid runner arguments');
const began = performance.now();
const child = spawn(command, args, { stdio: 'inherit', cwd: process.cwd() });
let deadlineHit = false;
let forceKill;
const deadline = setTimeout(() => {
  deadlineHit = true;
  child.kill('SIGTERM');
  forceKill = setTimeout(() => child.kill('SIGKILL'), 1000);
}, deadlineMs);
child.once('error', (error) => {
  console.error('child spawn error:', error.message);
});
child.once('close', (exitCode, signal) => {
  clearTimeout(deadline);
  clearTimeout(forceKill);
  console.log(JSON.stringify({ exitCode, signal, deadlineHit, elapsedMs: Math.round(performance.now() - began), childClosed: true }));
  process.exitCode = deadlineHit ? 124 : exitCode ?? 1;
});
