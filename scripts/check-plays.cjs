// Built-web interaction check with synthetic API data; no real users, sessions or game moves.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path');
const {spawn}=require('node:child_process'),{createRequire}=require('node:module');
const webRequire=createRequire(path.resolve('apps/web/package.json'));
const {chromium}=require(process.argv[2]||'playwright');
let vote=null,round=42,connected=true,requests=0;
const profile={username:'ExampleGame',display_name:'Example Game',avatar:null,linked:true,deleted:false,faction:null};
const server=http.createServer(async(req,res)=>{
  const url=new URL(req.url,'http://127.0.0.1'),signed=!!req.headers.cookie;
  let raw='';for await(const part of req)raw+=part;const body=raw?JSON.parse(raw):{};
  let data={},status=200;
  if(url.pathname==='/api/auth/me'){if(signed)data={username:'ExampleViewer',email_verified:true,faction:'aetheron'};else status=401;}
  else if(url.pathname.endsWith('/plays')){if(req.method==='POST'){assert(signed&&connected);assert.equal(body.round,round);assert.equal(vote,null);vote=body.command;requests++;}const now=new Date();data={game:'Synthetic game',round,closes_at:new Date(now.getTime()+4500),server_time:now,connected,input_mode:'rl',votes:vote?[{command:vote,votes:1}]:[],my_vote:signed?vote:null,can_vote:signed,last_command:null,last_chosen_at:null};}
  else if(url.pathname.endsWith('/resolve'))data={username:profile.username};
  else if(url.pathname==='/api/channels/ExampleGame')data={channel:{...profile,plays:true,bio:'A game',banner:null,mood_emoji:'',status_text:'',joined_at:new Date(),follower_count:0,following_count:0,links:[],song:null,live:false},viewer:{signed_in:signed,is_owner:false,following:false,blocked:false,interaction_blocked:false},tabs:{},header:{},war_council:{members:[],unavailable_count:0},wall_preview:{pinned:[],latest:[],viewer:{}},schedule_next:{items:[]}};
  else if(url.pathname.endsWith('/live'))data={live:false};
  else if(url.pathname.endsWith('/card'))data=profile;
  else if(url.pathname.endsWith('/suggestions'))data={items:[]};
  else if(url.pathname==='/api/me/following')data={items:[],next_cursor:null};
  else if(url.pathname==='/api/auth/config')data={providers:[],development:true,turnstile_site_key:''};
  else if(url.pathname.endsWith('/chat'))data={messages:[],emotes:[],pinned:null,viewer:{signed_in:signed,can_chat:signed}};
  else if(url.pathname==='/api/me/alerts')data={};
  else{status=404;data={error:'Not found'};}
  res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(data));
});
(async()=>{let next,browser;try{
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(8080,'127.0.0.1',resolve);});
  let output='';next=spawn(process.execPath,[webRequire.resolve('next/dist/bin/next'),'start','--hostname','127.0.0.1','--port','13001'],{cwd:path.resolve('apps/web'),env:{...process.env,API_INTERNAL_ORIGIN:'http://127.0.0.1:8080'},stdio:['ignore','pipe','pipe'],windowsHide:true});
  next.stdout.on('data',c=>output+=c);next.stderr.on('data',c=>output+=c);
  for(let i=0;i<80&&!output.includes('Ready in');i++){assert.equal(next.exitCode,null,output);await new Promise(r=>setTimeout(r,250));}assert.match(output,/Ready in/);
  browser=await chromium.launch({headless:true,channel:'msedge'});
  for(const width of [1440,390,320]){
    vote=null;connected=true;round++;const context=await browser.newContext({viewport:{width,height:900}}),page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.goto('http://127.0.0.1:13001/ExampleGame/live');await page.getByText('Synthetic game',{exact:true}).waitFor();
    const up=page.getByRole('button',{name:'Vote up',exact:true});assert(await up.isDisabled());
    await context.addCookies([{name:'sver_dev',value:'synthetic-viewer',url:'http://127.0.0.1:13001'}]);await page.reload();await up.waitFor();
    await page.waitForFunction(()=>!document.querySelector('[aria-label="Vote up"]').disabled);await up.focus();await page.keyboard.press('ArrowUp');
    await page.getByText('You voted UP.',{exact:true}).waitFor();assert(await up.isDisabled());assert.equal(vote,'up');
    vote=null;round++;await page.waitForFunction(()=>!document.querySelector('[aria-label="Vote up"]').disabled);
    connected=false;await page.getByText('Reconnecting',{exact:true}).waitFor();assert(await up.isDisabled());
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);assert.deepEqual(errors,[]);
    if(width<960){const controls=await page.getByRole('region',{name:'Play together'}).boundingBox(),chat=await page.getByRole('heading',{name:/Chat/}).boundingBox();assert(controls.y<chat.y,'Controls must precede chat on phones');}
    await page.evaluate(()=>scrollTo(0,0));
    if(process.env.SVER_SCREENSHOTS)await page.screenshot({path:`tmp/plays-${width}.png`,fullPage:true});
    await context.close();console.log(`${width}px: guest gate, keyboard vote, one vote, new round, disconnected controls, no overflow/errors`);
  }
  assert.equal(requests,3);
}finally{if(browser)await browser.close();if(next)next.kill();server.closeAllConnections();await new Promise(r=>server.close(r));}})().catch(e=>{console.error(e);process.exitCode=1;});
