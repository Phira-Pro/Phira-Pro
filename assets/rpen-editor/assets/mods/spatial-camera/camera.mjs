
export const CAMERA_ID = 'spatial-camera';
export const DEFAULT_FOV = 2 * Math.atan(450 / 1000) * 180 / Math.PI;
export const AXES = Object.freeze([
  { type: 'position', id: 'x', name: '位置 X', unit: '', fallback: 0, min: -50000, max: 50000, slider: [-1350,1350], step: 1, drag: 1, presets: [-675,0,675] },
  { type: 'position', id: 'y', name: '位置 Y', unit: '', fallback: 0, min: -50000, max: 50000, slider: [-900,900], step: 1, drag: 1, presets: [-450,0,450] },
  { type: 'position', id: 'z', name: '位置 Z', unit: '', fallback: 1000, min: -50000, max: 50000, slider: [-3000,3000], step: 1, drag: 4, presets: [650,1000,1800] },
  { type: 'rotation', id: 'yaw', name: '偏航', unit: '°', fallback: 0, min: -3600, max: 3600, slider: [-80,80], step: .1, drag: .15, presets: [-25,0,25] },
  { type: 'rotation', id: 'pitch', name: '俯仰', unit: '°', fallback: 0, min: -89, max: 89, slider: [-75,75], step: .1, drag: .15, presets: [-25,0,25] },
  { type: 'rotation', id: 'roll', name: '画面倾斜', unit: '°', fallback: 0, min: -3600, max: 3600, slider: [-180,180], step: .1, drag: .2, presets: [-30,0,30] },
  { type: 'lens', id: 'fov', name: '视角', unit: '°', fallback: DEFAULT_FOV, min: 10, max: 150, slider: [10,120], step: .1, drag: .1, presets: [30,DEFAULT_FOV,75] },
]);
export const DEFAULT_POSE = Object.freeze(Object.fromEntries(AXES.map(axis => [axis.id, axis.fallback])));
export const round = value => Math.round(value * 1000) / 1000;
export function clampAxis(axis, value) { return Math.max(axis.min, Math.min(axis.max, Number.isFinite(value) ? value : axis.fallback)); }
export function poseValues(pose = {}) { return Object.fromEntries(AXES.map(axis => [axis.id, clampAxis(axis, pose[axis.id] ?? axis.fallback)])); }
const radians = value => value * Math.PI / 180;
export function cameraBasis(pose) {
  const yaw = radians(pose.yaw), pitch = radians(pose.pitch), roll = radians(pose.roll);
  const cy=Math.cos(yaw),sy=Math.sin(yaw),cp=Math.cos(pitch),sp=Math.sin(pitch),cr=Math.cos(roll),sr=Math.sin(roll);
  const right=[cy,0,sy],up=[-sy*sp,cp,cy*sp];
  return { right: right.map((value,index)=>value*cr+up[index]*sr), up: up.map((value,index)=>value*cr-right[index]*sr), forward: [sy*cp,sp,-cy*cp] };
}
export class PlaneCamera {
  constructor(pose, view) {
    this.pose=poseValues(pose);this.view=view;this.basis=cameraBasis(this.pose);this.position=[this.pose.x,this.pose.y,this.pose.z];
    this.focal=450/Math.tan(radians(this.pose.fov)/2);this.near=1;this.far=150000;
  }
  cameraPoint(point) { const x=point.x-this.pose.x,y=point.y-this.pose.y,z=(point.z??0)-this.pose.z,{right,up,forward}=this.basis;return {x:x*right[0]+y*right[1]+z*right[2],y:x*up[0]+y*up[1]+z*up[2],depth:x*forward[0]+y*forward[1]+z*forward[2]}; }
  depth(point){const {forward}=this.basis;return(point.x-this.pose.x)*forward[0]+(point.y-this.pose.y)*forward[1]+((point.z??0)-this.pose.z)*forward[2];}
  project(point) {
    const p=this.cameraPoint(point);if(p.depth<this.near-1e-7||p.depth>this.far+1e-7)return null;
    p.depth=Math.max(this.near,Math.min(this.far,p.depth));
    return{x:this.view.width/2+p.x*this.focal/p.depth*this.view.scale,y:this.view.height/2-p.y*this.focal/p.depth*this.view.scale,depth:p.depth};
  }
  planeAt(x,y) {
    const dx=(x-this.view.width/2)/(this.view.scale*this.focal),dy=-(y-this.view.height/2)/(this.view.scale*this.focal);
    const direction=this.basis.forward.map((value,index)=>value+dx*this.basis.right[index]+dy*this.basis.up[index]);
    if(Math.abs(direction[2])<1e-7)return null;
    const distance=-this.pose.z/direction[2];if(distance<this.near||distance>this.far)return null;
    return{x:this.pose.x+direction[0]*distance,y:this.pose.y+direction[1]*distance,depth:distance};
  }
  clip(points) {
    const depths=points.map(point=>this.depth(point));
    if(depths.every(depth=>depth>=this.near&&depth<=this.far))return points;
    if(points.length===2){const [a,b]=points,[start,end]=depths,delta=end-start;let low=0,high=1;if(Math.abs(delta)<1e-12)return[];const near=(this.near-start)/delta,far=(this.far-start)/delta;low=Math.max(low,Math.min(near,far));high=Math.min(high,Math.max(near,far));if(low>high)return[];const at=t=>t===0?a:t===1?b:{x:a.x+(b.x-a.x)*t,y:a.y+(b.y-a.y)*t,z:(a.z??0)+((b.z??0)-(a.z??0))*t};return[at(low),at(high)];}
    let result=points.map(point=>({...point}));
    for(const [boundary,sign] of [[this.near,1],[this.far,-1]]) {
      const output=[];
      for(let index=0;index<result.length;index++) {
        const a=result[index],b=result[(index+1)%result.length],da=(this.depth(a)-boundary)*sign,db=(this.depth(b)-boundary)*sign;
        if(da>=0)output.push(a);
        if((da>=0)!==(db>=0)){const t=da/(da-db);output.push({x:a.x+(b.x-a.x)*t,y:a.y+(b.y-a.y)*t,z:(a.z??0)+((b.z??0)-(a.z??0))*t});}
      }
      result=output;
    }
    return result;
  }
  affine(raw){
    const {right,up,forward}=this.basis;if(Math.abs(forward[0])+Math.abs(forward[1])>1e-10)return null;
    const origin=this.cameraPoint({x:0,y:0});if(origin.depth<this.near||origin.depth>this.far)return null;
    const scale=this.focal/origin.depth*this.view.scale,a=scale*right[0],b=-scale*up[0],c=-scale*right[1],d=scale*up[1];
    const x=raw.width/2-(raw.center?.x??0),y=raw.height/2+(raw.center?.y??0);
    return[a,b,c,d,this.view.width/2+scale*origin.x-a*x-c*y,this.view.height/2-scale*origin.y-b*x-d*y];
  }
  bounds() {
    const v=this.view.viewport, points=[];
    for(let y=0;y<=4;y++)for(let x=0;x<=4;x++){const point=this.planeAt(v.left+v.width*x/4,v.top+v.height*y/4);if(point)points.push(point);}
    if(!points.length)return null;
    const x=points.map(point=>point.x),y=points.map(point=>point.y);
    // Rays near the horizon can approach infinity. Keep a finite world window and GPU texture budget.
    const cx=(Math.min(...x)+Math.max(...x))/2,cy=(Math.min(...y)+Math.max(...y))/2;
    const width=Math.min(80000,Math.max(200,Math.max(...x)-Math.min(...x))*1.08),height=Math.min(80000,Math.max(200,Math.max(...y)-Math.min(...y))*1.08);
    return {left:cx-width/2,bottom:cy-height/2,width,height,center:{x:cx,y:cy}};
  }
}

export function neutralPose(pose) { return AXES.every(axis=>Math.abs(pose[axis.id]-axis.fallback)<1e-8); }
export function orbitPose(yaw=0,pitch=0,distance=1000,roll=0) {
  const basis=cameraBasis({...DEFAULT_POSE,yaw,pitch,roll:0});
  return {...DEFAULT_POSE,x:-basis.forward[0]*distance,y:-basis.forward[1]*distance,z:-basis.forward[2]*distance,yaw,pitch,roll};
}
export const POSES=Object.freeze({front:{name:'正视',pose:DEFAULT_POSE},orbitLeft:{name:'左侧环绕',pose:orbitPose(28,12)},orbitRight:{name:'右侧环绕',pose:orbitPose(-28,12)},top:{name:'俯视',pose:orbitPose(0,-38)},tilt:{name:'侧倾',pose:{...DEFAULT_POSE,roll:22}},close:{name:'拉近',pose:{...DEFAULT_POSE,z:700}},wide:{name:'拉远',pose:{...DEFAULT_POSE,z:1600}}});
export function valueDraft(chart,type,column,index,field,value) {
  const data=chart.globalEvents,stored=data?.types?.[type],source=stored?.columns?.[column];if(!source?.events[index])throw Error('目标摄像机事件已不存在');
  const current=source.events[index],event={...current,[field]:value,...(current.inst?{start:value,end:value}:{})};
  return {...chart,globalEvents:{...data,types:{...data.types,[type]:{...stored,columns:{...stored.columns,[column]:{...source,events:source.events.map((item,at)=>at===index?event:item)}}}}}};
}
