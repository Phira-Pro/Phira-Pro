import { CAMERA_ID } from './camera.mjs';

export const cameraFollowsChart = chart => chart.globalEvents?.types?.[CAMERA_ID+'/lens']?.settings?.followChart === true;

/** Keep chart options with the camera's attributed, undoable global-event data. */
export function withCameraFollow(chart, manifest, enabled) {
  if (typeof enabled !== 'boolean') throw Error('跟随谱面需要布尔值');
  if (cameraFollowsChart(chart) === enabled) return chart;
  const id=CAMERA_ID+'/lens',data=chart.globalEvents??{version:1,types:{}},previous=data.types[id];
  const type={...previous,name:previous?.name??`摄像机镜头 · ${manifest.name}（${manifest.author}）`,
    source:{kind:'mod',id:manifest.id,name:manifest.name,author:manifest.author,version:manifest.version},
    columns:previous?.columns??{fov:{name:'视角',timelineLabel:'视角',valueType:'number',events:[]}},
    settings:{...previous?.settings,followChart:enabled}};
  return {...chart,META:{...chart.META,RPEVersion:Math.max(240,chart.META.RPEVersion??0)},globalEvents:{...data,types:{...data.types,[id]:type}}};
}
