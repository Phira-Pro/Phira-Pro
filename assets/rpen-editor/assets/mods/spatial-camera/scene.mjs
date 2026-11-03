import { PlaneCamera, cameraBasis, DEFAULT_POSE, poseValues } from './camera.mjs';

const point=a=>({x:a[0],y:a[1],z:a[2]});
const array=p=>[p.x,p.y,p.z??0];
const dot=(a,b)=>a.reduce((sum,value,index)=>sum+value*b[index],0);
const add=(a,b,s=1)=>a.map((value,index)=>value+b[index]*s);
const cross=(a,b)=>[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];
const distance=(p,a,b)=>{const dx=b.x-a.x,dy=b.y-a.y,t=Math.max(0,Math.min(1,((p.x-a.x)*dx+(p.y-a.y)*dy)/(dx*dx+dy*dy||1)));return Math.hypot(p.x-a.x-t*dx,p.y-a.y-t*dy);};
export function observerCamera(view,width,height){
  const basis=cameraBasis({...DEFAULT_POSE,yaw:view.yaw,pitch:view.pitch}),position=add(view.target,basis.forward,-view.distance);
  return new PlaneCamera({...DEFAULT_POSE,x:position[0],y:position[1],z:position[2],yaw:view.yaw,pitch:view.pitch,fov:55},{width,height,scale:Math.min(width/1150,height/800),viewport:{left:0,top:0,width,height}});
}
export function screenAxisDelta(dx,dy,start,end,length){const x=end.x-start.x,y=end.y-start.y;return(dx*x+dy*y)*length/Math.max(1,x*x+y*y);}
export function lookAtOrigin(pose){const length=Math.hypot(pose.x,pose.y,pose.z);if(length<1)return poseValues(pose);return poseValues({...pose,yaw:Math.atan2(-pose.x,pose.z)*180/Math.PI,pitch:Math.asin(-pose.y/length)*180/Math.PI});}
export function cameraFrustum(pose,aspect=1.5){
  const center=[pose.x,pose.y,pose.z],basis=cameraBasis(pose),depth=260,half=depth*Math.tan(pose.fov*Math.PI/360);
  const corners=[[-1,-1],[1,-1],[1,1],[-1,1]].map(([x,y])=>point(add(add(add(center,basis.forward,depth),basis.right,x*half*aspect),basis.up,y*half)));
  return{center:point(center),corners,basis};
}
function ray(camera,p){const v=camera.view,dx=(p.x-v.width/2)/(v.scale*camera.focal),dy=-(p.y-v.height/2)/(v.scale*camera.focal);return camera.basis.forward.map((value,i)=>value+camera.basis.right[i]*dx+camera.basis.up[i]*dy);}
function ringAngle(camera,p,center,u,v){const normal=cross(u,v),direction=ray(camera,p),denominator=dot(direction,normal);if(Math.abs(denominator)<1e-7)return null;const amount=dot(add(center,camera.position,-1),normal)/denominator;if(amount<=0)return null;const relative=add(add(camera.position,direction,amount),center,-1);return Math.atan2(dot(relative,v),dot(relative,u));}

/** A separate observer views the chart plane and both editable scene cameras. */
export class CameraScene {
  constructor(canvas,{change,select}){
    this.canvas=canvas;canvas.cameraScene=this;this.change=change;this.select=select;this.mode='move';this.active='end';this.view={yaw:-35,pitch:-24,distance:1600,target:[0,0,350]};this.poses={start:{...DEFAULT_POSE},end:{...DEFAULT_POSE}};this.listeners=[];
    const listen=(name,fn,options)=>{canvas.addEventListener(name,fn,options);this.listeners.push(()=>canvas.removeEventListener(name,fn,options));};
    listen('pointerdown',event=>this.down(event));listen('pointermove',event=>this.move(event));listen('pointerup',()=>{this.drag=null;canvas.style.cursor='grab';});listen('pointercancel',()=>{this.drag=null;});listen('contextmenu',event=>event.preventDefault());
    listen('wheel',event=>{event.preventDefault();this.view.distance=Math.max(450,Math.min(120000,this.view.distance*Math.exp(event.deltaY*.001)));this.draw();},{passive:false});
    this.observer=new ResizeObserver(()=>this.draw());this.observer.observe(canvas);
  }
  set(poses,active,texture){this.poses=poses;this.active=active;this.texture=texture??this.texture;this.draw();}
  frame(){const rect=this.canvas.getBoundingClientRect();return{width:rect.width,height:rect.height};}
  draw(){
    const {width,height}=this.frame();if(!width||!height)return;const ratio=devicePixelRatio||1;this.canvas.width=Math.round(width*ratio);this.canvas.height=Math.round(height*ratio);const c=this.canvas.getContext('2d');c.setTransform(ratio,0,0,ratio,0,0);c.fillStyle='#101b27';c.fillRect(0,0,width,height);this.camera=observerCamera(this.view,width,height);this.handles=[];this.bodies=[];
    const polygon=[{x:-675,y:-450,z:0},{x:675,y:-450,z:0},{x:675,y:450,z:0},{x:-675,y:450,z:0}];
    const projected=polygon.map(p=>this.camera.project(p));if(projected.every(Boolean)){c.beginPath();projected.forEach((p,i)=>i?c.lineTo(p.x,p.y):c.moveTo(p.x,p.y));c.closePath();c.fillStyle='#213645';c.fill();if(this.texture)this.drawTexture(c,this.texture);}
    c.globalAlpha=.4;for(let x=-675;x<=675;x+=135)this.line(c,{x,y:-450,z:0},{x,y:450,z:0},'#7294a5');for(let y=-450;y<=450;y+=90)this.line(c,{x:-675,y,z:0},{x:675,y,z:0},'#7294a5');c.globalAlpha=1;
    for(let i=0;i<4;i++)this.line(c,polygon[i],polygon[(i+1)%4],'#7eacbb',1.5);
    for(const [v,color,label]of [[[200,0,0],'#ed7878','X'],[[0,200,0],'#83dbaa','Y'],[[0,0,200],'#81b6ff','Z']]){this.line(c,{x:0,y:0,z:0},point(v),color,2);const p=this.camera.project(point(v));if(p){c.font='12px system-ui';c.fillStyle=color;c.fillText(label,p.x+5,p.y-5);}}
    c.setLineDash([5,5]);this.line(c,this.poses.start,this.poses.end,'#a0b7c3',1);c.setLineDash([]);
    this.drawCamera(c,this.active==='start'?'end':'start',false);this.drawCamera(c,this.active,true);
    c.fillStyle='#adc1d0';c.font='12px system-ui';c.fillText('谱面平面 · Z=0',12,height-15);c.fillText(this.mode==='move'?'拖动彩色轴或摄像机本体移动':'拖动彩色圆环旋转',12,23);
  }
  line(c,a,b,color,width=1){const points=this.camera.clip([a,b]);if(points.length<2)return;const start=this.camera.project(points[0]),end=this.camera.project(points[1]);if(!start||!end)return;c.strokeStyle=color;c.lineWidth=width;c.beginPath();c.moveTo(start.x,start.y);c.lineTo(end.x,end.y);c.stroke();}
  drawTexture(c,texture){
    const n=6,w=texture.width,h=texture.height;c.save();c.globalAlpha=.75;
    const vertex=(x,y)=>({screen:this.camera.project({x:-675+1350*x/n,y:450-900*y/n,z:0}),uv:{x:w*x/n,y:h*y/n}});
    for(let y=0;y<n;y++)for(let x=0;x<n;x++){const a=vertex(x,y),b=vertex(x+1,y),d=vertex(x,y+1),e=vertex(x+1,y+1);for(const triangle of [[a,b,e],[a,e,d]]){
      if(triangle.some(p=>!p.screen))continue;const [p,q,r]=triangle,s=p.uv,t=q.uv,u=r.uv,den=(t.x-s.x)*(u.y-s.y)-(u.x-s.x)*(t.y-s.y);if(!den)continue;
      const a1=((q.screen.x-p.screen.x)*(u.y-s.y)-(r.screen.x-p.screen.x)*(t.y-s.y))/den,b1=((q.screen.y-p.screen.y)*(u.y-s.y)-(r.screen.y-p.screen.y)*(t.y-s.y))/den;
      const a2=((r.screen.x-p.screen.x)*(t.x-s.x)-(q.screen.x-p.screen.x)*(u.x-s.x))/den,b2=((r.screen.y-p.screen.y)*(t.x-s.x)-(q.screen.y-p.screen.y)*(u.x-s.x))/den;
      c.save();c.beginPath();c.moveTo(p.screen.x,p.screen.y);c.lineTo(q.screen.x,q.screen.y);c.lineTo(r.screen.x,r.screen.y);c.closePath();c.clip();c.transform(a1,b1,a2,b2,p.screen.x-a1*s.x-a2*s.y,p.screen.y-b1*s.x-b2*s.y);c.drawImage(texture,0,0);c.restore();
    }}c.restore();
  }
  drawCamera(c,key,active){
    const pose=this.poses[key],shape=cameraFrustum(pose,this.aspect??1.5),center=this.camera.project(shape.center);if(!center)return;const color=active?'#7ee9ff':'#84939e';c.globalAlpha=active?1:.32;
    for(let i=0;i<4;i++){this.line(c,shape.center,shape.corners[i],color,active?1.6:1);this.line(c,shape.corners[i],shape.corners[(i+1)%4],color,1.3);}
    c.fillStyle=color;c.strokeStyle='#07111c';c.lineWidth=2;c.beginPath();c.roundRect(center.x-10,center.y-8,20,16,4);c.fill();c.stroke();c.font='bold 12px system-ui';c.fillStyle=color;c.fillText(key==='start'?'起始摄像机':'结束摄像机',center.x+15,center.y+(key==='start'?29:-13));this.bodies.push({key,center});c.globalAlpha=1;
    if(!active)return;const base=array(pose),length=Math.max(100,Math.min(1800,this.camera.cameraPoint(pose).depth/(this.camera.focal*this.camera.view.scale)*65));
    if(this.mode==='move')for(const [id,vector,color]of [['x',[1,0,0],'#f78383'],['y',[0,1,0],'#80e1ad'],['z',[0,0,1],'#8eb9ff']]){
      const p=this.camera.project(point(add(base,vector,length)));if(!p)continue;this.line(c,pose,point(add(base,vector,length)),color,3);c.fillStyle=color;c.beginPath();c.arc(p.x,p.y,5,0,Math.PI*2);c.fill();c.font='bold 12px system-ui';c.fillText(id.toUpperCase(),p.x+7,p.y-6);this.handles.push({kind:'axis',id,start:center,end:p,length});
    }
    else for(const [id,u,v,color]of [['yaw',[1,0,0],[0,0,1],'#80e1ad'],['pitch',[0,1,0],[0,0,1],'#f78383'],['roll',shape.basis.right,shape.basis.up,'#8eb9ff']]){
      const radius=length*.8,points=Array.from({length:73},(_,i)=>point(add(add(base,u,Math.cos(i/72*Math.PI*2)*radius),v,Math.sin(i/72*Math.PI*2)*radius))),screens=points.map(p=>this.camera.project(p));
      for(let i=1;i<points.length;i++)this.line(c,points[i-1],points[i],color,2.5);this.handles.push({kind:'ring',id,u,v,center:base,screens});
    }
  }
  pointer(event){const r=this.canvas.getBoundingClientRect();return{x:event.clientX-r.left,y:event.clientY-r.top};}
  down(event){
    event.preventDefault();this.canvas.focus();const p=this.pointer(event),pose=this.poses[this.active];let handle;
    if(event.button===0){
      handle=this.handles.find(h=>h.kind==='axis'&&Math.hypot(p.x-h.start.x,p.y-h.start.y)>14&&distance(p,h.start,h.end)<9||h.kind==='ring'&&h.screens.some((b,i)=>i&&b&&h.screens[i-1]&&distance(p,h.screens[i-1],b)<8));
      if(!handle){const body=[...this.bodies].reverse().find(b=>Math.hypot(p.x-b.center.x,p.y-b.center.y)<20);if(body){if(body.key!==this.active){this.select(body.key);return;}handle={kind:'free'};}}
    }
    this.drag={point:p,pose:{...pose},handle,view:structuredClone(this.view),kind:handle?'edit':event.button===1||event.shiftKey?'pan':'orbit'};
    if(handle?.kind==='ring')this.drag.angle=ringAngle(this.camera,p,handle.center,handle.u,handle.v);
    this.canvas.setPointerCapture(event.pointerId);this.canvas.style.cursor='grabbing';
  }
  move(event){
    const d=this.drag;if(!d)return;const p=this.pointer(event),dx=p.x-d.point.x,dy=p.y-d.point.y,camera=this.camera;
    if(d.kind==='orbit'){this.view.yaw=d.view.yaw+dx*.3;this.view.pitch=Math.max(-80,Math.min(80,d.view.pitch-dy*.3));this.draw();return;}
    if(d.kind==='pan'){const scale=d.view.distance/(camera.focal*camera.view.scale);this.view.target=add(add(d.view.target,camera.basis.right,-dx*scale),camera.basis.up,dy*scale);this.draw();return;}
    const h=d.handle,pose={...d.pose};
    if(h.kind==='axis'){const amount=screenAxisDelta(dx,dy,h.start,h.end,h.length);pose[h.id]+=event.shiftKey?Math.round(amount/25)*25:amount;}
    else if(h.kind==='free'){const depth=camera.cameraPoint(d.pose).depth,scale=depth/(camera.focal*camera.view.scale),position=add(add(array(d.pose),camera.basis.right,dx*scale),camera.basis.up,-dy*scale);[pose.x,pose.y,pose.z]=position;}
    else{const angle=ringAngle(camera,p,h.center,h.u,h.v);let amount=d.angle===null||angle===null?(dx-dy)*.3:Math.atan2(Math.sin(angle-d.angle),Math.cos(angle-d.angle))*180/Math.PI;pose[h.id]+=event.shiftKey?Math.round(amount/5)*5:amount;}
    this.change(poseValues(pose));
  }
  frameAll(){const poses=Object.values(this.poses),center=poses.reduce((sum,p)=>add(sum,array(p),1/3),[0,0,0]);this.view.target=center;this.view.distance=Math.max(1600,...poses.map(p=>Math.hypot(...add(array(p),center,-1))*1.8));this.draw();}
  dispose(){this.observer.disconnect();this.listeners.forEach(remove=>remove());delete this.canvas.cameraScene;this.drag=null;}
}
