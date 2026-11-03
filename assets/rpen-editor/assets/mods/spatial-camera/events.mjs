import { EventTrack } from '@rpen/core/events.mjs';
import { beatValue, fromNumber } from '@rpen/core/beat.mjs';
import { AXES, CAMERA_ID, poseValues } from './camera.mjs';

export class CameraTracks {
  sample(chart,tempo,seconds) {
    if(this.source!==chart.globalEvents||this.tempo!==tempo) {
      this.source=chart.globalEvents;this.tempo=tempo;this.tracks=new Map();this.hasEvents=false;
      for(const axis of AXES){const events=chart.globalEvents?.types?.[CAMERA_ID+'/'+axis.type]?.columns?.[axis.id]?.events??[];this.hasEvents ||= events.length>0;this.tracks.set(axis.id,new EventTrack(events,tempo,1,axis.fallback));}
    }
    return poseValues(Object.fromEntries(AXES.map(axis=>{
      const track=this.tracks.get(axis.id);const value=!track.events.length||seconds<track.events[0].start?axis.fallback:track.value(seconds);
      return[axis.id,value];
    })));
  }
}
export function transitionChanges(document,start,end,first,last,easingType=1) {
  if(!Number.isFinite(start)||!Number.isFinite(end)||end<=start)throw Error('结束拍必须晚于开始拍');
  return AXES.map(axis=>{
    const type=CAMERA_ID+'/'+axis.type,events=document.read(type,axis.id);
    if(events.some(event=>Math.max(start,beatValue(event.startTime))<Math.min(end,beatValue(event.endTime))-1e-8))throw Error(axis.name+'在此范围已有事件，请先调整时间范围');
    return {type,column:axis.id,events:[...events,{startTime:fromNumber(start),endTime:fromNumber(end),start:first[axis.id],end:last[axis.id],easingType,easingLeft:0,easingRight:1,bezier:0,bezierPoints:[0,0,1,1],inst:0}]};
  });
}
