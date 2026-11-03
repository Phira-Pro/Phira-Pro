export const SLOT_COUNT=9;
export const PICKUP_DELAY=2.3;
export function createInventory(){return{slots:['pickaxe',...Array(SLOT_COUNT-1).fill(null)],selected:0,experience:0};}
export function freeSlot(inventory){return inventory.slots.findIndex((item,index)=>index>0&&item===null);}
export function reserveItem(inventory,id){const index=freeSlot(inventory);if(index>0)inventory.slots[index]=id;return index;}
export function takeItem(inventory){const {selected,slots}=inventory;if(selected===0)return null;const id=slots[selected];slots[selected]=null;return id;}
export function experienceLevel(points){let level=0;let remaining=Math.max(0,Math.floor(points));while(remaining>=7+level*2){remaining-=7+level*2;level++;}return{level,progress:remaining/(7+level*2)};}
export function dropPose(drop,time,width,height){
  const age=Math.max(0,time-drop.birth);const floor=Math.max(52,height-126);const x=Math.max(26,Math.min(width-26,drop.x+drop.drift*Math.min(age,0.7)));
  const landingTime=drop.y>=floor?0:(100+Math.sqrt(10000+2480*(floor-drop.y)))/1240;const fall=Math.min(age,landingTime);const natural=drop.y-100*fall+620*fall*fall;const landed=age>=landingTime;
  const y=landed?floor+Math.sin((age-0.6)*3)*4:Math.min(floor,natural);
  return{x,y,scale:1+((drop.initialScale??1)-1)*Math.max(0,1-age/0.32),rotation:age*95,landed};
}

export const EXPERIENCE_REST=0.5;
export function advanceExperienceOrb(orb,time,dt,target,width,height){
  const age=time-orb.birth;if(age<0)return false;
  orb.originX??=orb.x;orb.originY??=orb.y;
  const floor=Math.max(24,height-126),gravity=900;
  const fallTime=orb.originY>=floor?0:(-orb.vy+Math.sqrt(orb.vy*orb.vy+2*gravity*(floor-orb.originY)))/gravity;
  const restAge=age-fallTime;
  if(restAge<EXPERIENCE_REST){
    const fall=Math.min(age,fallTime);orb.x=Math.max(6,Math.min(width-6,orb.originX+orb.vx*fall));
    orb.y=restAge<0?Math.min(floor,orb.originY+orb.vy*fall+gravity*fall*fall/2):floor-(restAge<0.2?Math.sin(restAge/0.2*Math.PI)*7:0);
    orb.phase=restAge<0?'fall':'rest';return false;
  }
  orb.phase='attract';const factor=1-Math.exp(-Math.max(0,dt)*12);orb.x+=(target.x-orb.x)*factor;orb.y+=(target.y-orb.y)*factor;
  return Math.hypot(orb.x-target.x,orb.y-target.y)<12;
}
