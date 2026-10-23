"""Isolated desktop QA data and loopback phira-mp server. Never uses real credentials."""
import argparse, asyncio, contextlib, json, pathlib, shutil, struct, threading, wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

REPO = pathlib.Path(__file__).resolve().parents[2]
ROOT = REPO / 'target/mp-desktop-qa'
STAMP = '2026-10-23T00:00:00Z'
DROP_ONCE = False
def leb(n):
    b = bytearray()
    while True:
        v = n & 127; n >>= 7; b.append(v | (128 if n else 0))
        if not n: return bytes(b)
def text(s):
    b=s.encode(); return leb(len(b))+b
def frame(b): return leb(len(b))+b
def chart():
    event=lambda v:[dict(startTime=0,endTime=1000000,start=v,end=v,start2=0.5,end2=0.5)]
    return dict(formatVersion=3,offset=0,numOfNotes=6,judgeLineList=[dict(bpm=120,
        notesAbove=[dict(type=k,time=t,holdTime=0,floorPosition=t/64,positionX=0,speed=1) for t,k in [(128,1),(192,2),(256,4),(320,1),(384,2),(448,4)]],
        notesBelow=[],speedEvents=[dict(startTime=0,endTime=1000000,value=1)],judgeLineMoveEvents=event(.5),judgeLineRotateEvents=event(0),judgeLineDisappearEvents=event(1))])
def setup():
    ROOT.mkdir(parents=True,exist_ok=True)
    shutil.copy2(REPO/'target/release/phira-main.exe',ROOT/'phira-main.exe')
    shutil.copytree(REPO/'assets',ROOT/'assets',dirs_exist_ok=True)
    d=ROOT/'data/charts/custom/mp-qa';d.mkdir(parents=True,exist_ok=True)
    from PIL import Image
    Image.new('RGB',(1280,720),(28,55,91)).save(d/'background.png')
    with wave.open(str(d/'song.wav'),'wb') as f:
        f.setnchannels(1);f.setsampwidth(2);f.setframerate(8000);f.writeframes(bytes(8000*2*10))
    (d/'chart.json').write_text(json.dumps(chart()),encoding='utf-8')
    info=dict(id=7,name='Multiplayer Regression',level='IN Lv.1',difficulty=1,charter='QA',composer='QA',illustrator='QA',format='pgr',music='song.wav',chart='chart.json',illustration='background.png',aspectRatio=16/9,intro='',chartUpdated=STAMP,created=STAMP,updated=STAMP)
    (d/'info.yml').write_text(json.dumps(info),encoding='utf-8')
    data=dict(charts=[dict(**info,local_path='custom/mp-qa',mods='')],language='zh-CN',tokens=['test','test'],read_tos_and_policy=True,multiplayer_address='127.0.0.1:26317',
        config=dict(apiUrl='http://127.0.0.1:26318',offlineMode=False,volumeBgm=0,volumeMusic=0,volumeSfx=0,fullscreenMode=False,adjust_time_new=False))
    (ROOT/'data/data.json').write_text(json.dumps(data),encoding='utf-8')
    print(ROOT,flush=True)
class Http(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path=='/chart/7':
            base='http://127.0.0.1:26318'
            value=dict(id=7,name='Multiplayer Regression',level='IN Lv.1',difficulty=1,charter='QA',composer='QA',illustrator='QA',description='',ranked=False,reviewed=True,stable=False,stableRequest=False,illustration=base+'/background.png',preview=base+'/song.wav',file=base+'/chart.zip',uploader=1,created=STAMP,updated=STAMP,chartUpdated=STAMP,tags=[],rating=None)
            payload=json.dumps(value).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(payload)
        elif self.path=='/message/has-unread':
            self.send_response(200);self.end_headers();self.wfile.write(b'false')
        else:
            self.send_response(404);self.end_headers()
    def log_message(self,*args):pass
async def receive(r):
    n=0
    for i in range(5):
        b=(await r.readexactly(1))[0];n|=(b&127)<<(i*7)
        if not b&128:return await r.readexactly(n)
    raise ValueError('frame')
async def handle(r,w):
    global DROP_ONCE
    room=True;selected=7
    async def send(*payloads):
        w.write(b''.join(map(frame,payloads)));await w.drain()
    try:
        assert await r.readexactly(1)==b'\1'
        while True:
            b=await receive(r);tag=b[0]
            with (ROOT/'loopback-events.jsonl').open('a',encoding='utf-8') as log:log.write(json.dumps(dict(command=tag,bytes=len(b)))+'\n')
            if tag==0:await send(b'\0')
            elif tag==1:
                assert b==b'\1\4test','Only synthetic QA credentials accepted'
                user=struct.pack('<i',1)+text('QA')+bytes([0])
                snapshot=text('qa-room')+bytes([0,1])+struct.pack('<i',7)+bytes([1,0,0,1,0,1])+struct.pack('<i',1)+user
                await send(bytes([1,1])+user+bytes([1])+snapshot)
                if DROP_ONCE:
                    DROP_ONCE=False
                    # A reproducible transport failure, not a gameplay/end request.
                    asyncio.get_running_loop().call_later(1,w.close)
            elif tag==5:
                room=True;await send(bytes([8,1]))
            elif tag==6:
                room=True;await send(bytes([9,1,0,0,1])+struct.pack('<i',1)+text('QA')+bytes([0,0]))
            elif tag==7:
                room=False;await send(bytes([11,1]))
            elif tag==10:
                selected=struct.unpack('<i',b[1:])[0]
                await send(bytes([5,5])+struct.pack('<i',1)+text('Multiplayer Regression')+struct.pack('<i',selected),bytes([6,0,1])+struct.pack('<i',selected),bytes([14,1]))
            elif tag==11:
                assert selected==7 and room
                await send(bytes([15,1]),bytes([5,10]),bytes([6,2]))
            elif tag==15:
                await send(bytes([19,1]),bytes([5,12]),bytes([6,0,1])+struct.pack('<i',selected))
            elif tag==14:raise AssertionError('Pro must never send Played to the legacy result provider')
            elif tag==2:await send(bytes([5,0])+struct.pack('<i',1)+b[1:],bytes([2,1]))
            elif tag in [8,9]:await send(bytes([5,14 if tag==8 else 15,b[1]]),bytes([12 if tag==8 else 13,1]))
            elif tag in [3,4]:pass
            else:await send(bytes([{12:16,13:17}.get(tag,19),1]))
    except (asyncio.IncompleteReadError,ConnectionResetError):pass
    finally:
        w.close()
        with contextlib.suppress(OSError):await w.wait_closed()
async def serve():
    ROOT.mkdir(parents=True,exist_ok=True)
    http=ThreadingHTTPServer(('127.0.0.1',26318),Http);threading.Thread(target=http.serve_forever,daemon=True).start()
    server=await asyncio.start_server(handle,'127.0.0.1',26317)
    print('QA only: MP 127.0.0.1:26317, HTTP 127.0.0.1:26318',flush=True)
    async with server:await server.serve_forever()
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--setup',action='store_true');p.add_argument('--serve',action='store_true');p.add_argument('--drop-once',action='store_true');a=p.parse_args()
    DROP_ONCE=a.drop_once
    if a.setup:setup()
    if a.serve:asyncio.run(serve())
