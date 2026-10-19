// Generate independent expected values by running the pinned RPEN implementation.
// Usage: node scripts/qa/rpen_reference.mjs [path to RPEN checkout]
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const root=path.resolve(process.argv[2]??'target/reference-rpen-latest');
const load=file=>import(pathToFileURL(path.join(root,file)).href);
const {compileTrajectory}=await load('src/core/curve-trajectory.mjs');
const {PlaneCamera,DEFAULT_FOV}=await load('assets/mods/spatial-camera/camera.mjs');
const {TempoMap}=await load('src/core/tempo.mjs');
const options=[
  {},
  {mode:'polar',radiusExpression:'100+30*cos(3*theta)',angleStart:'-pi',angleEnd:'5*pi',tangentRotation:true},
  {xExpression:'300*(2*t-1)',yExpression:'120*sin(6*pi*t)',easingX:9,easingY:3,trimStart:.15,trimEnd:.85,rotation:28,scaleX:1.2,scaleY:-.7,alignStart:true,startX:50,startY:-30},
  {xExpression:'length*(t-.5)',yExpression:'amplitude*(sin(2*pi*waves*t)+randomness*(.5*sin(13*pi*t+seed)+.3*cos(23*pi*t+2*seed)))',parameters:{length:600,amplitude:100,waves:2},seed:19,randomness:.6,tangentRotation:true},
  {shape:'star',parameters:{radius:200,points:7,inner:.33},rotationExpression:'20*sin(4*pi*t)'},
  {shape:'random-polygon',parameters:{radius:300,points:9},seed:3,randomness:.6},
  {xExpression:'t<.5?pow(t,2)*200:lerp(50,100,clamp((t-.5)*2,0,1))',yExpression:'abs(sin(2*pi*t))*100',rotationExpression:'floor(t*4)*90'},
];
const progress=[0,.0001,.03125,.125,.333,.5,.75,.9999,1];
const trajectories=options.map(options=>{const sample=compileTrajectory(options);return{options,samples:progress.map(p=>({p,...sample(p)}))};});
const cameras=[];
for(const aspect of [4/3,16/9,22/9])for(const pose of [
  {x:0,y:0,z:1000,yaw:0,pitch:0,roll:0,fov:DEFAULT_FOV},
  {x:250,y:-100,z:800,yaw:20,pitch:15,roll:30,fov:60},
  {x:0,y:0,z:-1000,yaw:180,pitch:0,roll:0,fov:DEFAULT_FOV},
]){
  const width=1200,height=width/aspect,scale=height/900;
  const camera=new PlaneCamera(pose,{width,height,scale});
  const samples=[[-.6,-.3],[0,0],[.7,.4]].map(([x,y])=>{
    const projected=camera.project({x:x*675,y:y*450*aspect});
    // Phira's RPE canvas maps X and Y to the full fitted viewport separately.
    // Convert the upstream pixel projection back to those chart coordinates.
    return {x,y,expected:projected?{x:(projected.x-width/2)/(scale*675),y:-(projected.y-height/2)/(scale*450*aspect)}:null};
  });
  cameras.push({aspect,pose,samples});
}
const bpm=[{bpm:120,startTime:[0,0,1]},{bpm:180,startTime:[4,0,1]},{bpm:90,startTime:[8,0,1]}];
const tempo=new TempoMap(bpm);
const timings=[.5,1,2].map(factor=>({factor,values:[0,2,4,6,10].map(beat=>({beat,time:tempo.seconds(beat,factor)}))}));
const result={source:'kclg-YSJ/rpe-next@1ee0fc3e3e434696f00cbb51d4107379913a6490',trajectories,cameras,bpm,timings};
const output='prpr/tests/fixtures/rpen-extensions/reference.json';fs.mkdirSync(path.dirname(output),{recursive:true});fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n');
console.log(output);
