// Generate expected values by executing the fixed upstream implementation.
// Usage: node scripts/qa/rpen_0131_reference.mjs target/reference-rpen-0131
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';

const revision = '7b46dc0923deee6d10d14a9dc5adf92e8cecc66f';
const root = path.resolve(process.argv[2] ?? 'target/reference-rpen-0131');
if (execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== revision) throw Error('Unexpected upstream revision');
const load = file => import(pathToFileURL(path.join(root, file)).href);
const { bezier } = await load('src/core/easing.mjs');
const { NoiseAreaRuntime, createNoiseArea } = await load('src/core/noise-areas.mjs');
const { TempoMap } = await load('src/core/tempo.mjs');
const points = [[0,0,0,1],[1,0,1,1],[1,0,1,0],[0,1,0,0],[1,1,0,0],[0.2,-4,0.8,6],[0.2341,0.1,0.77,0.9],[0.2349,0.1,0.77,0.9]];
const progress = [-0.01,0,1e-12,0.00001,0.01,0.1,0.25,0.5,0.8,0.99,0.99999,1-1e-12,1,1.01];
const interval = (a,b,readyFlash=0.5) => ({ startTime:[a,0,1], endTime:[b,0,1], readyFlash });
const cases = [
  { range:[0,12], intervals:[interval(4,4)] },
  { range:[0,12], intervals:[interval(4,4,1.25)] },
  { range:[0,12], intervals:[interval(4,4,0)] },
  { range:[2,12], intervals:[interval(3,9),interval(5,5)] },
  { range:[2,12], intervals:[interval(1,1),interval(13,13)] },
  { range:[2,12], intervals:[interval(0,2),interval(12,14)] },
  { range:[2,12], intervals:[interval(12,12)] },
  { range:[2,12], intervals:[interval(4,4,0),interval(4,6)] },
  { range:[2,12], intervals:[interval(4,6),interval(4,4,0)] },
];
const times = [0.5,0.75,1,1.49,1.5,1.5+1/12,1.75,1.9,2,2.1,2.5,3,4,4.5,5.5,5.75,6];
const tempo = new TempoMap([{ bpm:120, startTime:[0,0,1] }]);
const result = {
  source:`kclg-YSJ/rpe-next@${revision}`, version:'0.13.1',
  bezier:points.map(points => ({ points, samples:progress.map(progress => ({ progress, value:bezier(progress,points) })) })),
  noise:cases.map(test => {
    const area = createNoiseArea(...test.range); area.activeIntervals = test.intervals;
    const runtime = new NoiseAreaRuntime(area,tempo);
    return { area, samples:times.map(time => { const value=runtime.sample(time); return { time, state:value.state, mix:value.activationMix }; }) };
  }),
};
const output = 'prpr/tests/fixtures/rpen-extensions/v0131.json';
fs.writeFileSync(output, JSON.stringify(result,null,2)+'\n');
console.log(output);
