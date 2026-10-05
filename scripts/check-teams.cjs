// Run after pnpm build: node scripts/check-teams.cjs [path to installed playwright].
// Synthetic loopback fixtures; Postgres tests cover real permissions and persistence.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),http=require('node:http');
const {spawn,spawnSync}=require('node:child_process'),{createRequire}=require('node:module');
const webRequire=createRequire(path.resolve('apps/web/package.json'));
const {chromium}=require(process.argv[2]||'playwright');
// Loopback-only, synthetic layout fixtures; API behavior is covered by the Postgres suite.

const {WebSocketServer}=webRequire('next/dist/compiled/ws');
const at='2026-10-05T12:00:00Z';
const people=['Wren','Oak','Nova','Flint'].map((username,i)=>({username,display_name:username,avatar:null,linked:true,deleted:false,faction:['aetheron','myria','glint','aetheron'][i],live:true,host:i===0}));
const guild={id:'guild-fixture',slug:'workshop',name:'The Workshop',tag:'WORK',tagline:'Different banners. Shared ambition.',about:'We build games, shape wood, paint worlds and learn together. Bring a project, find your people, and make something worth sharing.',status:'ACTIVE',recruiting:true,verified:true,avatar:'/api/fixture-media/emblem.png',banner:null,revision:0};
const community=['guild_application','guild_decision','guild_invite','squad_invite'].map(kind=>({kind,site:true,push:true}));
const state={role:'leader',following:false,badge:true,muted:false,requests:[{id:'application',user:{...people[2],live:false},message:'I stream landscape painting on weekends. I would love to join your monthly maker sessions.',created_at:at}]};
const messages=people.map((author,i)=>({id:`message-${i}`,seq:i+1,author:{...author,guild:{slug:guild.slug,name:guild.name,tag:guild.tag,image:guild.avatar}},body:['Welcome to the workshop. What are you making today?','Finishing the last joints on this bookshelf.','Working on a small mountain landscape.','Testing the controller in my game.'][i],created_at:at,role:i===0?'owner':null,mentions:[],reply:null}));
const snap={messages,pinned:null,emotes:[]};
const server=http.createServer(async(req,res)=>{
 const u=new URL(req.url,'http://localhost'),p=u.pathname;
 if(p.startsWith('/api/fixture-media/')){const name=path.basename(p);if(!/^(sample\.m3u8|sample\d+\.ts|emblem\.png)$/.test(name)){res.writeHead(404);return res.end();}const file=path.join('tmp/teams-media',name);if(!fs.existsSync(file)){res.writeHead(404);return res.end();}res.writeHead(200,{'content-type':name.endsWith('m3u8')?'application/vnd.apple.mpegurl':name.endsWith('.png')?'image/png':'video/mp2t','cache-control':'no-store'});return fs.createReadStream(file).pipe(res);}
 let body='';for await(const chunk of req)body+=chunk;let b={};try{b=JSON.parse(body)}catch{};
 const signed=!!req.headers.cookie, outsider=req.headers.cookie?.includes('applicant'), role=signed&&!outsider?state.role:null;
 let data;
 if(p==='/api/auth/me'){if(!signed){res.writeHead(401,{'content-type':'application/json'});return res.end('{}');}data={username:outsider?'Nova':'Wren',faction:'aetheron',email_verified:true,mfa_enabled:true};}
 else if(p==='/api/me/alerts')data={notifications:2};
 else if(p==='/api/me/following')data={items:people.slice(1).map(user=>({user:{...user,direct_follow:false},followed_at:at,guilds:[{name:guild.name,slug:guild.slug}]})),next_cursor:null};
 else if(p==='/api/guilds')data={items:[guild,{...guild,id:'guild-two',slug:'nightshift',name:'Night Shift',tag:'NITE',tagline:'Late streams. Long projects.',avatar:null,verified:false}],next_offset:null};
 else if(p==='/api/me/guilds')data={items:[{...guild,role}],can_create:true,badge:guild.id};
 else if(p==='/api/me/guilds/badge'){state.badge=!!b.guild_id;data={saved:true};}
 else if(p==='/api/guilds/workshop/actions'){if(b.action==='follow'||b.action==='unfollow')state.following=b.action==='follow';if(b.action==='accept'||b.action==='decline')state.requests=[];data={saved:true};}
 else if(p==='/api/guilds/workshop')data={guild,members:people.map((user,i)=>({user,live:i<3,role:i===0?'leader':i===1?'officer':'member',title:i===1?'Master carpenter':'',joined_at:at})),schedule:people.slice(0,3).map((user,i)=>({user,occurrence:{start_at:`2026-10-0${6+i}T18:00:00Z`,end_at:`2026-10-0${6+i}T20:00:00Z`,label:['Building a tiny platform game','Finishing the walnut bookshelf','Painting landscapes'][i],kind:'event',live:false}})),viewer:{signed_in:signed,role,can_manage:!!role,following:state.following,muted:state.muted,badge:state.badge,invited:outsider,application:null},management:role?{applications:state.requests,events:[{id:1,action:'joined',actor:'Wren',subject:'Oak',created_at:at}],blocks:['BlockedExample'],verification:{status:'APPROVED',note:'Organization confirmed.'}}:null};
 else if(p==='/api/me/squads')data={current:null,live:true,invites:[{id:'merged',host:'Oak',mode:'MERGED',expires_at:'2026-10-05T12:10:00Z'}]};
 else if(/^\/api\/squads\/[^/]+$/.test(p))data={id:p.split('/').at(-1),mode:p.endsWith('separate')?'SEPARATE':'MERGED',ended:false,members:people,host:signed&&!outsider,joined:signed&&!outsider,invited:outsider,pending:[]};
 else if(p.endsWith('/chat/moderation'))data={role:role?'owner':null,restrictions:[]};
 else if(p.endsWith('/chat')){if(req.method==='POST'){data={message:{...messages[0],id:b.id,seq:10,body:b.body}}}else data=snap;}
 else if(p.endsWith('/live')){const name=p.split('/')[3];data={live:true,broadcast_id:`fixture-${name}`,state:'LIVE',title:`${name}'s creative workshop`,category:'Game development',viewers:0,is_owner:false,playback:{preferred:'hls',webrtc:null,hls:'/api/fixture-media/sample.m3u8'}};}
 else if(p.endsWith('/live/beat'))data={recorded:true};
 else if(p==='/api/me/hosting')data={accept_hosts:true,accept_raids:true,auto_host:true,auto_list:['Oak']};
 else if(p==='/api/admin/appeals')data={items:[]};
 else if(p==='/api/admin/guilds')data={items:[{...guild,emblem_pending:true,verification_status:'OPEN',evidence:'A registered creator collective. Organization website: example.test',note:'',content:{image:guild.avatar,banner:null,tagline:guild.tagline,about:guild.about}}]};
 else if(p==='/api/me/notifications/settings')data={site:true,push:true,email:false,push_devices:0,push_key:'',community};
 else if(p==='/api/me/notifications')data={items:[{id:'n1',kind:'guild_application',created_at:at,read:false,payload:{title:'New guild application',body:guild.name,url:'/g/workshop/settings'}},{id:'n2',kind:'squad_invite',created_at:at,read:false,payload:{title:'Co-stream invitation',body:'Oak invited you to a co-stream.',url:'/squads/merged'}}]};
 else if(p==='/api/me/reports')data={items:[],next_cursor:null};
 else if(p==='/api/me/standing')data={strikes:[],unread:0};
 else if(req.method!=='GET')data={saved:true};
 else {res.writeHead(404,{'content-type':'application/json'});return res.end(JSON.stringify({error:'Fixture route missing'}));}
 res.writeHead(200,{'content-type':'application/json','cache-control':'no-store'});res.end(JSON.stringify(data));
});
const sockets=new WebSocketServer({server});sockets.on('connection',ws=>{ws.send(JSON.stringify({type:'snapshot',...snap}));ws.on('message',raw=>{const m=JSON.parse(raw);ws.send(JSON.stringify({type:'ack',id:m.id,message:{...messages[0],id:m.id,seq:20,body:m.body}}))})});


(async()=>{let next,browser;try{
 fs.mkdirSync('tmp/teams-media',{recursive:true});
 let result=spawnSync('ffmpeg',['-v','error','-f','lavfi','-i','testsrc2=size=320x180:rate=12','-f','lavfi','-i','sine=frequency=220:sample_rate=48000','-t','45','-c:v','libx264','-preset','ultrafast','-pix_fmt','yuv420p','-g','24','-c:a','aac','-b:a','64k','-f','hls','-hls_time','2','-hls_list_size','0','-y','tmp/teams-media/sample.m3u8'],{windowsHide:true});assert.equal(result.status,0,result.stderr?.toString());
 result=spawnSync('ffmpeg',['-v','error','-f','lavfi','-i','testsrc2=size=112x112','-frames:v','1','-y','tmp/teams-media/emblem.png'],{windowsHide:true});assert.equal(result.status,0,result.stderr?.toString());
 await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(8080,'127.0.0.1',resolve);});
 let output='';next=spawn(process.execPath,[webRequire.resolve('next/dist/bin/next'),'start','--hostname','127.0.0.1','--port','13001'],{cwd:path.resolve('apps/web'),env:{...process.env,API_INTERNAL_ORIGIN:'http://127.0.0.1:8080'},stdio:['ignore','pipe','pipe'],windowsHide:true});
 next.stdout.on('data',c=>output+=c);next.stderr.on('data',c=>output+=c);
 for(let i=0;i<80&&!output.includes('Ready in');i++){assert.equal(next.exitCode,null,output);await new Promise(r=>setTimeout(r,250));}assert.match(output,/Ready in/);
 browser=await chromium.launch({headless:true,channel:'msedge'});
 for(const width of [1440,390,320]){
  const context=await browser.newContext({viewport:{width,height:950}});await context.addCookies([{name:'sver_dev',value:'synthetic-teams-preview',url:'http://127.0.0.1:13001'}]);
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
  for(const route of ['/guilds','/g/workshop','/g/workshop/settings','/studio/guilds','/studio/squads','/squads/merged','/squads/separate','/admin/guilds','/settings/notifications','/notifications','/following']){
   await page.goto(`http://127.0.0.1:13001${route}`);await page.waitForTimeout(1200);
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,`${width} ${route} overflow`);
   assert.equal(await page.locator('main h1').count(),1,`${route}: title`);
   if(route==='/g/workshop'){await page.getByRole('button',{name:'Follow guild',exact:true}).click();await page.getByRole('button',{name:'Unfollow guild',exact:true}).waitFor();await page.getByRole('button',{name:'Unfollow guild',exact:true}).click();}
   if(route.startsWith('/squads/')){
    await page.waitForFunction(()=>[...document.querySelectorAll('video')].length===4&&[...document.querySelectorAll('video')].every(v=>v.currentTime>0),null,{timeout:20000});
    assert.equal(await page.locator('video').evaluateAll(vs=>vs.every(v=>v.muted)),true);
    await page.getByRole('button',{name:'Listen to Oak',exact:true}).click();
    await page.waitForFunction(()=>[...document.querySelectorAll('video')].filter(v=>!v.muted).length===1);
    await page.getByRole('button',{name:'Listen to Nova',exact:true}).click();
    await page.waitForFunction(()=>[...document.querySelectorAll('video')].filter(v=>!v.muted).length===1&&document.querySelectorAll('video')[2].muted===false);
    await page.getByRole('button',{name:'Mute all',exact:true}).click();
    if(route.endsWith('separate'))await page.getByLabel('Channel chat').selectOption('Flint');
    await page.getByRole('textbox',{name:'Message',exact:true}).fill('Together we build');await page.getByRole('button',{name:'Send',exact:true}).click();await page.getByText('Together we build',{exact:true}).waitFor();
    const boxes=await page.locator('.squad-stream').evaluateAll(es=>es.map(e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y}}));if(width<600)assert(boxes[1].y>boxes[0].y);else assert(boxes[1].x>boxes[0].x);
   }
   if(process.env.SVER_SCREENSHOTS&&width!==320){await page.evaluate(()=>scrollTo(0,0));await page.screenshot({path:`tmp/teams-${route.replaceAll('/','_')}-${width}.png`,fullPage:true});}
  }
  await context.clearCookies();await page.goto('http://127.0.0.1:13001/g/workshop');await page.getByRole('link',{name:'Sign in to follow or apply'}).waitFor();assert.equal(await page.getByRole('link',{name:'Manage guild',exact:true}).count(),0);
  await context.addCookies([{name:'sver_dev',value:'synthetic-applicant',url:'http://127.0.0.1:13001'}]);await page.reload();await page.getByLabel('Tell the team about your streams').fill('I stream painting and want to join.');await page.getByRole('button',{name:'Send application',exact:true}).click();await page.getByText('Saved.',{exact:true}).waitFor();
  assert.deepEqual(errors,[]);console.log(`${width}px: 11 pages, no overflow or page errors; four HLS players advance; one audible stream; separate/shared chat sends; guest gate and applicant form.`);await context.close();
 }

}finally{if(browser)await browser.close();if(next)next.kill();for(const ws of sockets.clients)ws.terminate();server.closeAllConnections();await new Promise(r=>server.close(r));}})().catch(e=>{console.error(e);process.exitCode=1;});
