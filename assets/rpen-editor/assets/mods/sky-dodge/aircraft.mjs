// The outer locator is decorative; the inner circle remains the collision radius.
export function drawAircraft(context,point,radius,time,invincible=false,invincibilityElapsed=time){
  context.save();context.translate(point.x,point.y);
  const color=invincible?'#ffe19a':'#70e8ff';
  const blinkDim=invincible&&Math.floor(Math.max(0,invincibilityElapsed)*8)%2===1;
  const locator=radius+10+Math.sin(time*3)*1.2;
  const glow=context.createRadialGradient(0,0,radius*0.5,0,0,locator+10);
  glow.addColorStop(0,'#041321aa');glow.addColorStop(0.45,'#07121b70');glow.addColorStop(0.7,color+'38');glow.addColorStop(1,color+'00');
  context.fillStyle=glow;context.beginPath();context.arc(0,0,locator+10,0,Math.PI*2);context.fill();
  context.fillStyle='#06111bd9';context.beginPath();context.arc(0,0,radius+2,0,Math.PI*2);context.fill();
  context.lineCap='round';
  context.beginPath();for(let index=0;index<4;index++){const angle=Math.PI/4+index*Math.PI/2;context.moveTo(Math.cos(angle-0.23)*locator,Math.sin(angle-0.23)*locator);context.arc(0,0,locator,angle-0.23,angle+0.23);}
  context.strokeStyle='#051019';context.lineWidth=5;context.stroke();context.strokeStyle=color;context.lineWidth=2;context.stroke();
  context.save();context.globalAlpha=blinkDim?0.2:1;context.beginPath();context.arc(0,0,radius,0,Math.PI*2);context.strokeStyle='#031019';context.lineWidth=4;context.stroke();context.strokeStyle=color;context.lineWidth=1.4;context.stroke();context.restore();
  context.globalAlpha=blinkDim?0.12:1;
  context.beginPath();context.moveTo(0,-radius);context.lineTo(radius*0.86,radius*0.5);context.lineTo(radius*0.24,radius*0.28);context.lineTo(0,radius*0.85);context.lineTo(-radius*0.24,radius*0.28);context.lineTo(-radius*0.86,radius*0.5);context.closePath();
  context.strokeStyle='#020910';context.lineWidth=3;context.lineJoin='round';context.stroke();
  const body=context.createLinearGradient(0,-radius,0,radius);body.addColorStop(0,'#ffffff');body.addColorStop(0.55,'#f2fcff');body.addColorStop(1,'#8be9ff');context.fillStyle=body;
  context.shadowColor=color;context.shadowBlur=blinkDim?0:7;context.fill();context.shadowBlur=0;
  context.fillStyle='#075c7c';context.beginPath();context.ellipse(0,-radius*0.15,Math.max(1,radius*0.12),radius*0.28,0,0,Math.PI*2);context.fill();
  context.restore();
}
