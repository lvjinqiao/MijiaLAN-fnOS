#!/usr/bin/env python3
"""Loopback-only daemon integration test. Pass the native daemon binary as argv[1]."""
import json, os, socket, subprocess, sys, tempfile, threading, time, urllib.request, urllib.error
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
from pathlib import Path
urllib.request.install_opener(urllib.request.build_opener(urllib.request.ProxyHandler({})))
binary=Path(sys.argv[1]).resolve()
power=False
delay_next=False
received=[]
class DeviceAPI(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_GET(self):
        self.send_response(200);self.end_headers();self.wfile.write(json.dumps({'data':{'power':power}}).encode())
    def do_POST(self):
        global power,delay_next
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        received.append((self.path,self.headers.get('X-Device-Key'),body))
        if delay_next:
            delay_next=False
            def apply():
                global power
                power=body['on']
            threading.Timer(.7,apply).start()
        else:power=body['on']
        self.send_response(200);self.end_headers();self.wfile.write(b'{}')
mock=ThreadingHTTPServer(('127.0.0.1',0),DeviceAPI)
threading.Thread(target=mock.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='mijialan-test-') as folder:
    token='integration-test-token-12345'
    sock=socket.socket();sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close()
    env=dict(os.environ,MIJIALAN_DATA_DIR=folder,MIJIALAN_ADMIN_TOKEN=token,MIJIALAN_LISTEN=f'127.0.0.1:{port}')
    env.pop('MIJIALAN_SOCKET',None)
    for k in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy'):env.pop(k,None)
    def api(path,body=None,authorized=True):
        headers={'Content-Type':'application/json'}
        if authorized:headers['Authorization']='Bearer '+token
        req=urllib.request.Request(f'http://127.0.0.1:{port}/api/{path}',None if body is None else json.dumps(body).encode(),headers)
        try:
            with urllib.request.urlopen(req,timeout=12) as r:return r.status,json.load(r)
        except urllib.error.HTTPError as e:return e.code,json.load(e)
    def start():
        p=subprocess.Popen([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        for _ in range(100):
            if p.poll() is not None:raise RuntimeError(p.stderr.read().decode())
            try:api('status');return p
            except OSError:time.sleep(.05)
        raise RuntimeError('startup timeout')
    p=start()
    try:
        assert api('devices',authorized=False)[0]==401
        config={'id':'http-test','name':'API 插座','kind':'outlet','bridged':True,
            'on':{'type':'http','request':{'url':f'http://127.0.0.1:{mock.server_port}/power','method':'POST','headers':{'X-Device-Key':'test-secret'},'body':{'on':True}}},
            'off':{'type':'http','request':{'url':f'http://127.0.0.1:{mock.server_port}/power','method':'POST','body':{'on':False}}},
            'probe':{'type':'http','request':{'url':f'http://127.0.0.1:{mock.server_port}/status'},'pointer':'/data/power'},'interval_secs':5}
        assert api('custom',config)[0]==200
        assert api('status')[1]['homekit']['devices']['custom:http-test']['running'] is True
        assert api('homes')[1]==[]
        assert api('xiaomi/logout',{})[0]==200
        assert any(d['device']['did']=='custom:http-test' for d in api('devices')[1])
        result=api('device/refresh',{'did':'custom:http-test'});assert result[0]==200,result;assert result[1]['values']['2.1'] is False
        assert api('device/write',{'did':'custom:http-test','key':'2.1','value':True})[0]==200
        assert received==[('/power','test-secret',{'on':True})]
        assert api('device/refresh',{'did':'custom:http-test'})[1]['values']['2.1'] is True
        assert api('device/write',{'did':'custom:http-test','key':'2.1','value':'bad'})[0]==400
        assert api('custom',{'id':'../bad','name':'bad','probe':{'type':'report'}})[0]==400
        assert api('custom',{'id':'sensor','name':'温度','kind':'temperature','probe':{'type':'report'}})[0]==200
        assert api('custom/report',{'id':'sensor','value':21.5})[0]==200
        assert api('custom/report',{'id':'sensor','value':150})[0]==400
        assert api('custom/report',{'id':'http-test','value':False})[0]==400
        # WOL is sent to a local UDP receiver, never a physical computer.
        udp=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);udp.bind(('127.0.0.1',0));udp.settimeout(3)
        wol={'id':'wol-test','name':'测试电脑','on':{'type':'wol','mac':'00:11:22:33:44:55','broadcast':'127.0.0.1','port':udp.getsockname()[1]},'probe':{'type':'optimistic'}}
        assert api('custom',wol)[0]==200
        assert api('device/write',{'did':'custom:wol-test','key':'2.1','value':True})[0]==200
        packet=udp.recv(1024);assert packet==b'\xff'*6+bytes.fromhex('001122334455')*16;udp.close()
        assert api('device/write',{'did':'custom:wol-test','key':'2.1','value':False})[0]==400
        assert api('custom',{'id':'ping-test','name':'本地 Ping','probe':{'type':'ping','host':'127.0.0.1'}})[0]==200
        assert api('device/refresh',{'did':'custom:ping-test'})[1]['values']['2.1'] is True
        # Each selected device owns a distinct pairing identity and port.
        assert api('device/bridge',{'did':'custom:sensor','enabled':True})[0]==200
        bridges=api('status')[1]['homekit']['devices']
        first_uri=bridges['custom:http-test']['uri']
        assert first_uri != bridges['custom:sensor']['uri']
        assert bridges['custom:http-test']['port'] != bridges['custom:sensor']['port']
        assert bridges['custom:http-test']['accessories']==1
        assert bridges['custom:sensor']['accessories']==1
        assert api('homekit',{'enabled':True})[0]==400
        assert api('device/bridge',{'did':'custom:sensor','enabled':False})[0]==200
        bridges=api('status')[1]['homekit']['devices']
        assert 'custom:sensor' not in bridges
        assert bridges['custom:http-test']['uri']==first_uri
        # A delayed device response must update by prioritized readback, without a manual refresh.
        delay_next=True
        started=time.monotonic()
        assert api('device/write',{'did':'custom:http-test','key':'2.1','value':False})[0]==200
        assert api('device/state',{'did':'custom:http-test'})[1]['values']['2.1'] is True
        while time.monotonic()-started<4:
            if api('device/state',{'did':'custom:http-test'})[1]['values']['2.1'] is False:break
            time.sleep(.1)
        else:raise AssertionError('prioritized readback did not confirm delayed change')
        assert api('device/write',{'did':'custom:http-test','key':'2.1','value':True})[0]==200
        assert api('device/refresh',{'did':'custom:http-test'})[1]['values']['2.1'] is True
        # Password changes invalidate old credentials and survive restart even when the
        # install-time environment still contains the old default.
        old_token=token
        assert api('password',{'current_password':'wrong','new_password':'changed-password','confirm_password':'changed-password'})[0]==400
        assert api('password',{'current_password':token,'new_password':'changed-password','confirm_password':'different'})[0]==400
        assert api('password',{'current_password':token,'new_password':'changed-password','confirm_password':'changed-password'})[0]==200
        assert api('devices')[0]==401
        token='changed-password'
        assert api('devices')[0]==200
        # Verify settings and HTTP secrets survive restart without plaintext storage.
        assert b'test-secret' not in Path(folder,'credentials.enc').read_bytes()
        assert Path(folder,'master.key').stat().st_mode & 0o777==0o600
        p.terminate();p.wait(timeout=8);p=start()
        assert api('status')[1]['homekit']['devices']['custom:http-test']['uri']==first_uri
        assert 'custom:sensor' not in api('status')[1]['homekit']['devices']
        assert len(api('custom')[1])==4
        assert api('device/refresh',{'did':'custom:http-test'})[1]['values']['2.1'] is True
        assert api('custom/delete',{'id':'sensor'})[0]==200
        assert len(api('custom')[1])==3
        print('PASS: auth, HTTP control/readback, validation, report, WOL UDP, Ping, encrypted persistence, password change, old-token revocation, restart, delete')
    finally:
        p.terminate();p.wait(timeout=8);mock.shutdown()
