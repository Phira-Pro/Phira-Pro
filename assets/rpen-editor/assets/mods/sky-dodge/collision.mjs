export function nearestOnSegment(point,a,b){const dx=b.x-a.x,dy=b.y-a.y;const amount=Math.max(0,Math.min(1,((point.x-a.x)*dx+(point.y-a.y)*dy)/(dx*dx+dy*dy||1)));return{x:a.x+dx*amount,y:a.y+dy*amount};}
export function insidePolygon(point,polygon){let inside=false;for(let i=0,j=polygon.length-1;i<polygon.length;j=i++){const a=polygon[i],b=polygon[j];if((a.y>point.y)!==(b.y>point.y)&&point.x<(b.x-a.x)*(point.y-a.y)/(b.y-a.y)+a.x)inside=!inside;}return inside;}
export function circlePolygon(point,radius,polygon){if(insidePolygon(point,polygon))return true;for(let i=0;i<polygon.length;i++){const close=nearestOnSegment(point,polygon[i],polygon[(i+1)%polygon.length]);if(Math.hypot(close.x-point.x,close.y-point.y)<=radius)return true;}return false;}
export function rectangleOf(polygon){return{left:Math.min(...polygon.map(p=>p.x)),right:Math.max(...polygon.map(p=>p.x)),top:Math.min(...polygon.map(p=>p.y)),bottom:Math.max(...polygon.map(p=>p.y))};}
const near=(point,radius,bounds)=>point.x+radius>=bounds.left&&point.x-radius<=bounds.right&&point.y+radius>=bounds.top&&point.y-radius<=bounds.bottom;
export const NOTE_COLLISION_SCALE=0.85;
export function noteObstacles(entries){return entries.map(entry=>{
  let points=entry.polygon;
  if(points){const center=points.reduce((sum,p)=>({x:sum.x+p.x/points.length,y:sum.y+p.y/points.length}),{x:0,y:0});points=points.map(p=>({x:center.x+(p.x-center.x)*NOTE_COLLISION_SCALE,y:center.y+(p.y-center.y)*NOTE_COLLISION_SCALE}));}
  else{const dx=entry.x2-entry.x1,dy=entry.y2-entry.y1,length=Math.hypot(dx,dy);const nx=length?-dy/length:Math.cos(entry.rotation??0),ny=length?dx/length:Math.sin(entry.rotation??0);const half=(entry.collisionWidth??entry.radius)*NOTE_COLLISION_SCALE/2;points=[{x:entry.x1+nx*half,y:entry.y1+ny*half},{x:entry.x2+nx*half,y:entry.y2+ny*half},{x:entry.x2-nx*half,y:entry.y2-ny*half},{x:entry.x1-nx*half,y:entry.y1-ny*half}];}
  return{points,bounds:rectangleOf(points),source:entry};
});}
export function noiseObstacles(entries,width,height,scale){const groups=new Map();for(const entry of entries){if(entry.state!=='active')continue;const layer=entry.area.layerId??'default';if(!groups.has(layer))groups.set(layer,[]);const points=entry.points.map(p=>({x:width/2+p.x*scale,y:height/2-p.y*scale}));groups.get(layer).push({points,bounds:rectangleOf(points),invert:Boolean(entry.area.isInvert)});}return [...groups.values()];}
export function noiseCoverage(point,entries){let normal=false,inverted=0;for(const entry of entries)if(near(point,0,entry.bounds)&&insidePolygon(point,entry.points)){if(entry.invert)inverted++;else normal=true;}return normal!==(inverted===1);}
export function hitObstacles(point,radius,notes,noise){
  if(notes.some(entry=>near(point,radius,entry.bounds)&&circlePolygon(point,radius,entry.points)))return true;
  for(const group of noise){const nearby=group.filter(entry=>near(point,radius,entry.bounds));if(!nearby.length)continue;if(noiseCoverage(point,nearby))return true;
    // Test the circle rim plus exact closest edge points, retaining narrow and rotated obstacles.
    for(let step=0;step<16;step++){const angle=step*Math.PI/8;if(noiseCoverage({x:point.x+Math.cos(angle)*radius,y:point.y+Math.sin(angle)*radius},nearby))return true;}
    for(const entry of nearby)for(let i=0;i<entry.points.length;i++){const a=entry.points[i],b=entry.points[(i+1)%entry.points.length];const close=nearestOnSegment(point,a,b);if(Math.hypot(close.x-point.x,close.y-point.y)>radius)continue;const length=Math.hypot(b.x-a.x,b.y-a.y)||1;for(const side of [-1,1])if(noiseCoverage({x:close.x+side*(b.y-a.y)/length*0.01,y:close.y-side*(b.x-a.x)/length*0.01},nearby))return true;}
  }return false;
}
export function sweepPoints(previous,current,radius){if(!previous)return[current];const steps=Math.min(80,Math.max(1,Math.ceil(Math.hypot(current.x-previous.x,current.y-previous.y)/Math.max(2,radius*0.7))));return Array.from({length:steps},(_,i)=>({x:previous.x+(current.x-previous.x)*(i+1)/steps,y:previous.y+(current.y-previous.y)*(i+1)/steps}));}
