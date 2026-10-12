// Regenerate reference DATA with rpe-next's public export/runtime APIs.
// Usage: node scripts/verify_rpe_next.mjs target/rpe-next-audit
// Audited revision: 78dfc8b1748ad01a18d5f85767bd50c7bff50d38 (0.10.2).
// No upstream implementation is copied into this repository.
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
const revision = '78dfc8b1748ad01a18d5f85767bd50c7bff50d38';
const upstream = path.resolve(process.argv[2] ?? 'target/rpe-next-audit');
if (execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], { encoding:'utf8' }).trim() !== revision) throw Error('Unexpected upstream revision');
const mod = file => import(pathToFileURL(path.join(upstream, 'src', file)));
const { createChart, createNote, assertChart } = await mod('core/chart.mjs');
const { createNoiseArea, createNoiseEvent, NoiseAreaRuntime, withNoiseAreas } = await mod('core/noise-areas.mjs');
const { TempoMap } = await mod('core/tempo.mjs');
const { fromNumber: b } = await mod('core/beat.mjs');
const { createChartExport } = await mod('platform/files.mjs');
const output = 'prpr/tests/fixtures/rpe-next';
await fs.mkdir(output, { recursive:true });
await fs.mkdir('target/rpe-next-verification', { recursive:true });
const event = (a,z,t0,t1,anchor,extra={}) => ({...createNoiseEvent(a,z,t0,t1,anchor),...extra});
let chart = createChart();
chart.META = {...chart.META, name:'RPE Next Noise Compatibility', offset:320, song:'music.ogg', background:'background.png', level:'IN 15'};
chart.BPMList = [{bpm:120,startTime:b(0)},{bpm:180,startTime:b(4)},{bpm:90,startTime:b(10)}];
chart.judgeLineList[0].bpmfactor = 2; // Noise must NOT inherit this factor.
chart.judgeLineList[0].notes = [createNote(1,4,0),createNote(4,6,0),createNote(2,8,100,10),createNote(3,12,-100)];
const areas = [];
let a = createNoiseArea(1,16);
a.bottomLeft = {x:-300,y:-180}; a.topRight = {x:200,y:120};
a.activeIntervals = [{startTime:b(2),endTime:b(3),readyFlash:false},{startTime:b(5),endTime:b(7),readyFlash:1.2345},{startTime:b(11),endTime:b(15),readyFlash:true}];
a.moveXEvents = [event(-50,150,-2,3),event(150,-100,5,8,undefined,{bezier:true,bezierPoints:[0.21,-0.3,0.79,1.4]})];
a.moveYEvents = [event(-30,120,2,6,undefined,{easingType:23})];
a.rotateEvents = [event(0,40,-2,3,{x:180,y:75}),event(50,-30,5,9,{x:-140,y:210}),event(-30,95,11,14,{x:20,y:-30})];
a.scaleXEvents = [event(1,2,-2,3,{x:125,y:65}),event(2,-0.8,5,9,{x:-135,y:-90})];
a.scaleYEvents = [event(1,0.4,0,3,{x:45,y:170}),event(0.4,1.5,5,8,{x:-70,y:120})];
areas.push(a);
// All standard easing IDs, both full and clipped ranges. Include endpoints.
for (let type=1; type<=29; type++) for (const clipped of [false,true]) {
  a = createNoiseArea(-2,16);
  a.moveXEvents = [event(0,200,0,4,undefined,{easingType:type,easingLeft:clipped?0.13:0,easingRight:clipped?0.87:1})];
  a.activeIntervals = [{startTime:b(0),endTime:b(4),readyFlash:0.5}];
  areas.push(a);
}
// Same-time anchored transform chain, reference values and scale-zero reset.
a = createNoiseArea(0,16);
a.rotateEvents = [event(0,0,2,2,{x:130,y:50},{transformStart:0,transformEnd:35,transformStep:true}),
  event(35,35,2,2,{x:-95,y:30},{transformStart:35,transformEnd:70,transformStep:true}),
  event(70,130,2,6,{x:25,y:-70})];
a.scaleXEvents = [event(1,0,0,2,{x:200,y:100}),event(0,-2,3,6,{x:-150,y:40})];
a.activeIntervals = [{startTime:b(-5),endTime:b(2),readyFlash:-1},{startTime:b(7),endTime:b(20)}];
areas.push(a);
// No-track defaults and inverted corners.
a = createNoiseArea(0,16); a.isInvert=true;
a.bottomLeft={x:160,y:220}; a.topRight={x:-400,y:-200}; areas.push(a);
chart = withNoiseAreas(chart, areas);
assertChart(chart);
const json = createChartExport(chart,new Map(),'noise.json',{format:'json'});
await fs.writeFile(path.join(output,'chart.json'),new Uint8Array(await json.blob.arrayBuffer()));
const tempo = new TempoMap(chart.BPMList);
const beats = [-2.1,-2,-0.5,0,0.00001,0.13,0.5,1,1.75,2,2.00001,2.8,3,3.99999,4,4.5,5,6,7,8,9,10,11,12,14,15,15.99999,16];
// Include flash phases in seconds and exact half-open boundaries.
const times = [...new Set([...beats.map(v=>tempo.seconds(v)), ...[0,1/6,1/3,0.5,2/3,5/6,1].map(f=>tempo.seconds(5)-1.235+f*1.235)])];
const samples = areas.map((area,id) => {
  const runtime = new NoiseAreaRuntime(area,tempo);
  return times.map(time => { const s=runtime.sample(time); return {id,time,state:s.state,mix:s.activationMix,center:s.center,points:s.points}; });
}).flat();
await fs.writeFile(path.join(output,'samples.json'),JSON.stringify({revision,samples}));
// Small visible chart for the production GameScene JSON/PEZ import/render path.
let visual = structuredClone(chart);
visual.blockAreaList = [areas[0]];
for (let i=0;i<5;i++) {
  a=createNoiseArea(0,16);
  a.bottomLeft={x:-450+i*180,y:-300};a.topRight={x:-270+i*180,y:-180};
  a.activeIntervals=[{startTime:b(2+i),endTime:b(14),readyFlash:0.5}];
  visual.blockAreaList.push(a);
}
a=createNoiseArea(0,16);a.isInvert=true;a.bottomLeft={x:-90,y:-300};a.topRight={x:90,y:-180};
a.activeIntervals=[{startTime:b(4),endTime:b(14),readyFlash:0}];visual.blockAreaList.push(a);
const assets = new Map([
  ['music.ogg',new Uint8Array(await fs.readFile('assets/ending.ogg'))],
  ['background.png',new Uint8Array(await fs.readFile('assets/cloud_none.png'))],
]);
const frameTimes=[0.5,0.9,1.1,1.6,2.1,2.4,3.,4.2,5.,7.,9.];
const visualSamples=frameTimes.map(time=>({time,areas:visual.blockAreaList.map(area=>{
  const s=new NoiseAreaRuntime(area,tempo).sample(time);
  return {state:s.state,mix:s.activationMix,points:s.points,invert:!!area.isInvert};
}).filter(a=>!a.state.startsWith('hidden'))}));
await fs.writeFile('target/rpe-next-verification/visual-samples.json',JSON.stringify(visualSamples));
for (const format of ['json','pez']) {
  const e=createChartExport(visual,assets,'noise.json',{format});
  await fs.writeFile(`target/rpe-next-verification/noise.${format}`,new Uint8Array(await e.blob.arrayBuffer()));
}
await fs.writeFile('target/rpe-next-verification/music.ogg',assets.get('music.ogg'));
await fs.writeFile('target/rpe-next-verification/background.png',assets.get('background.png'));
await fs.writeFile('target/rpe-next-verification/info.txt','#\nName: RPE Next Noise Compatibility\nChart: noise.json\nSong: music.ogg\nPicture: background.png\nLevel: IN 15\n');
console.log(`Generated ${areas.length} areas, ${samples.length} upstream reference samples, JSON and PEZ exports (${revision}).`);
