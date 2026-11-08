#!/usr/bin/env python3
"""Local RTSP -> native camera -> paired HomeKit -> SRTP -> RTSP frame test.
Usage: test-camera.py DAEMON GO2RTC FFMPEG
No real camera or user HomeKit pairing is touched.
"""
import base64,json,os,re,socket,subprocess,sys,tempfile,time,urllib.request,urllib.parse
from pathlib import Path
urllib.request.install_opener(urllib.request.build_opener(urllib.request.ProxyHandler({})))
daemon,go2rtc,ffmpeg=map(lambda p:str(Path(p).resolve()),sys.argv[1:4])
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
with tempfile.TemporaryDirectory(prefix='mijialan-camera-') as tmp:
 root=Path(tmp);api_port,source_api,source_rtsp=port(),port(),port();processes=[]
 def call(path,body=None):
  req=urllib.request.Request(f'http://127.0.0.1:{api_port}/api/'+path,None if body is None else json.dumps(body).encode(),{'Authorization':'Bearer 12345','Content-Type':'application/json'})
  with urllib.request.urlopen(req,timeout=25) as r:return json.load(r)
 def launch(args,env=None):
  p=subprocess.Popen(args,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);processes.append(p);return p
 def wait_ready(url):
  for _ in range(100):
   try:urllib.request.urlopen(url,timeout=1).close();return
   except Exception:time.sleep(.05)
  raise AssertionError('service startup timed out')
 try:
  video=root/'source.mp4'
  subprocess.run([ffmpeg,'-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc=size=640x360:rate=15','-t','8','-c:v','libx264','-pix_fmt','yuv420p','-y',str(video)],check=True,timeout=20)
  source_config=root/'source.yaml';source_config.write_text(json.dumps({'api':{'listen':f'127.0.0.1:{source_api}'},'rtsp':{'listen':f'127.0.0.1:{source_rtsp}'},'webrtc':{'listen':''},'srtp':{'listen':f':{port()}'},'ffmpeg':{'bin':ffmpeg},'streams':{'test':'ffmpeg:'+str(video)+'#video=copy'}}))
  source=launch([go2rtc,'-config',str(source_config)]);wait_ready(f'http://127.0.0.1:{source_api}/api')
  env=dict(os.environ,MIJIALAN_DATA_DIR=str(root/'data'),MIJIALAN_LISTEN=f'127.0.0.1:{api_port}',MIJIALAN_CAMERA_BIN=go2rtc,MIJIALAN_FFMPEG_BIN=ffmpeg,MIJIALAN_ADMIN_TOKEN='12345')
  env.pop('MIJIALAN_SOCKET',None);env.pop('MIJIALAN_EXTRA_LISTEN',None)
  app=launch([daemon],env);wait_ready(f'http://127.0.0.1:{api_port}/')
  assert call('cameras')['available']
  url=f'rtsp://127.0.0.1:{source_rtsp}/test'
  call('cameras',{'id':'one','name':'测试摄像头一','url':url,'enabled':True})
  call('cameras',{'id':'two','name':'测试摄像头二','url':url,'enabled':True})
  print('Camera processes started',flush=True)
  pair_one=call('cameras/pairing',{'id':'one'});pair_two=call('cameras/pairing',{'id':'two'})
  assert pair_one['qr_svg']!=pair_two['qr_svg']
  print('Reading RTSP snapshot',flush=True)
  image=call('cameras/snapshot',{'id':'one'})['image'];assert base64.b64decode(image.split(',')[1]).startswith(b'\xff\xd8')
  print('RTSP snapshot passed',flush=True)
  print('Reading authenticated live preview',flush=True)
  req=urllib.request.Request(f'http://127.0.0.1:{api_port}/api/cameras/live',json.dumps({'id':'one'}).encode(),{'Authorization':'Bearer 12345','Content-Type':'application/json'})
  with urllib.request.urlopen(req,timeout=15) as response:
   live_data=response.read(100000)
  assert b'ftyp' in live_data[:64] and b'moof' in live_data and b'mdat' in live_data
  live_path=root/'preview.mp4';live_path.write_bytes(live_data)
  subprocess.run([ffmpeg,'-hide_banner','-loglevel','fatal','-i',str(live_path),'-frames:v','1','-y',str(root/'preview.jpg')],check=True,timeout=10)
  assert (root/'preview.jpg').read_bytes().startswith(b'\xff\xd8')
  print('Live preview decoded successfully',flush=True)

  config_path=root/'data/cameras/one.yaml';config=config_path.read_text()
  camera_port=int(re.search(r'listen: [\'"]?:(\d+)',config)[1]);device_id=re.search(r'device_id: ([A-F0-9:]+)',config)[1]
  # Only the HAP pairing endpoints are exposed on the LAN-facing camera listener.
  for endpoint in ['/api/config','/api/streams','/']:
   try:urllib.request.urlopen(f'http://127.0.0.1:{camera_port}'+endpoint,timeout=2);raise AssertionError('unprotected management endpoint')
   except urllib.error.HTTPError as error:assert error.code==404
  print('Pairing HomeKit camera',flush=True)
  body=urllib.parse.urlencode({'id':'paired-test','src':f'homekit://127.0.0.1:{camera_port}?device_id={device_id}','pin':pair_one['code']}).encode()
  urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{source_api}/api/homekit',body),timeout=10).close()
  assert call('cameras/pairing',{'id':'one'})['paired']
  assert not call('cameras/pairing',{'id':'two'})['paired']
  # This frame traverses the paired HomeKit/SRTP path, not the source RTSP URL.
  print('Reading HomeKit/SRTP frame',flush=True)
  output=root/'homekit-frame.jpg'
  subprocess.run([ffmpeg,'-hide_banner','-loglevel','error','-rtsp_transport','tcp','-i',f'rtsp://127.0.0.1:{source_rtsp}/paired-test?video','-frames:v','1','-y',str(output)],check=True,timeout=25)
  assert output.read_bytes().startswith(b'\xff\xd8')
  print('HomeKit/SRTP frame passed',flush=True)
  # Editing and daemon restart preserve the first camera's pairing.
  call('cameras',{'id':'one','name':'改名后的摄像头','enabled':True})
  assert call('cameras/pairing',{'id':'one'})['paired']
  app.terminate();app.wait(timeout=10);app=launch([daemon],env);wait_ready(f'http://127.0.0.1:{api_port}/')
  assert call('cameras/pairing',{'id':'one'})['paired']
  call('cameras/reset',{'id':'two','confirm':True})
  assert call('cameras/pairing',{'id':'one'})['paired']
  call('cameras',{'id':'two','name':'测试摄像头二','enabled':False})
  assert not next(c for c in call('cameras')['devices'] if c['id']=='two')['running']
  assert config_path.stat().st_mode&0o777==0o600
  assert 'rtsp://' not in json.dumps(call('cameras'))
  call('cameras/delete',{'id':'two'});assert len(call('cameras')['devices'])==1
  print('PASS: RTSP snapshot, two isolated camera QRs, HAP pairing, SRTP video frame, edit/restart pairing persistence, independent reset/disable/delete, management endpoint isolation')
 finally:
  for p in reversed(processes):
   if p.poll() is None:p.terminate()
   try:p.wait(timeout=10)
   except subprocess.TimeoutExpired:p.kill();p.wait()
