export const PICKUP_RADIUS=18;
export function drawPickup(context,pickup,time){
  const age=time-pickup.birth;const grow=Math.max(0,Math.min(1,age/0.3));
  const size=PICKUP_RADIUS*(1+0.12*Math.sin(time*3))*Math.sin(grow*Math.PI/2);
  const repair=pickup.kind==='repair';
  context.save();context.translate(pickup.x,pickup.y);context.rotate(time*0.3);
  context.shadowColor=repair?'#83efff':'#ffd887';context.shadowBlur=20;context.fillStyle=context.shadowColor;context.beginPath();
  if(repair){context.moveTo(0,-size);context.lineTo(size,0);context.lineTo(0,size);context.lineTo(-size,0);}
  else for(let index=0;index<10;index++){const angle=-Math.PI/2+index*Math.PI/5;const radius=index%2?size*0.48:size;const x=Math.cos(angle)*radius,y=Math.sin(angle)*radius;index?context.lineTo(x,y):context.moveTo(x,y);}
  context.closePath();context.lineJoin='round';context.strokeStyle='#132436';context.lineWidth=4;context.stroke();context.fill();context.shadowBlur=0;context.strokeStyle='#fff9';context.lineWidth=1;context.stroke();context.restore();
  if(repair){const arm=size/3;context.save();context.strokeStyle='#123246';context.lineWidth=2.5;context.lineCap='round';context.beginPath();context.moveTo(pickup.x-arm,pickup.y);context.lineTo(pickup.x+arm,pickup.y);context.moveTo(pickup.x,pickup.y-arm);context.lineTo(pickup.x,pickup.y+arm);context.stroke();context.restore();}
}
