import {getModHost} from '@rpen/api';
import {currentLocale,localizedValue} from '@rpen/core/i18n-core.mjs';
import {parseModPackage} from '@rpen/core/mod-package.mjs';
import {readZip,writeZip} from '@rpen/platform/archive.mjs';
import {storeMod,deleteMod,readMods} from '@rpen/platform/mods.mjs';
import {createModMarket} from './client.mjs';
import {marketSocket} from './transport.mjs';
import {nativeMarketView} from './view.mjs';
import {configureMarketText,marketText,runMarketTask} from './text.mjs';
export function activate(api){
  configureMarketText(currentLocale().locale,localizedValue);
  const {manager}=getModHost();
  const transport=marketSocket(()=>api.settings.get('socket-url'));
  const view=nativeMarketView(api,transport);
  const market=createModMarket({manager,parsePackage:parseModPackage,readZip,writeZip,storeMod,deleteMod,readMods,marketUrl:()=>api.settings.get('market-url'),mount:node=>api.mount(node),listen:(node,type,fn,options)=>api.listen(node,type,fn,options),transport,view,signal:api.signal,ownerId:api.id});
  api.settings.onChange(()=>market.refresh());
  api.patch(manager.runtime,'shortcut',original=>function(event){return view.isOpen()?false:original.call(this,event);});
  api.dispose(()=>market.dispose());
  api.commands.register({id:'market',title:marketText('openMarket'),run:()=>runMarketTask(()=>market.open())});
  api.patch(manager,'renderDetails',original=>function(...args){
    original.apply(this,args);const record=this.records.get(this.selected);
    if(record?.manifest.market?.id===record.manifest.id){const button=document.createElement('button');button.textContent=marketText('releasePage');button.onclick=()=>runMarketTask(()=>market.open(record.manifest.id)).catch(console.error);this.details.append(button);}
  });
}
