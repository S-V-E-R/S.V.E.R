// Built-web acceptance using synthetic API data on loopback only. No real sessions or external writes.
// corepack pnpm --dir apps/web build; node scripts/check-factions.cjs <path-to-playwright>
const assert = require('node:assert/strict');
const http = require('node:http');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { createRequire } = require('node:module');
const webRequire = createRequire(path.resolve('apps/web/package.json'));
const { chromium } = require(process.argv[2] || 'playwright');
const sides = ['myria', 'aetheron', 'glint'];
const genreNames = ['FPS & battle royale','Fighting','Sports & racing','Speedrunning','Crafting & making','RTS & MOBA','Strategy & 4X','Card & board','Puzzle & simulation','Art','Education & coding','Community events','MMOs & RPGs','Co-op & party','Cozy & sandbox','Music'];
const genres = genreNames.map((name,i) => ({ id: name.toLowerCase().replaceAll(' & ','_').replaceAll(' ','_'), name, position:i, home: sides[i<5 ? 0 : i<11 ? 1 : 2], holder: sides[i<5 ? 0 : i<11 ? 1 : 2], neighbors:[], scores: sides.map((faction,j)=>({ faction, influence: (i+1)*(j+1)*100, score:(i+1)*(j+1)*4 })) }));
const war = { season:{ number:1, starts_at:'2026-10-01T00:00:00Z', ends_at:'2027-01-01T00:00:00Z', next_starts_at:'2027-01-08T00:00:00Z', finished:false, winners:[] }, week:{ id:1, ends_at:'2026-10-12T00:00:00Z', completed:false }, genres, scoreboard:sides.map((faction,i)=>({faction,territories:i===1?6:5,genre_weeks:0})), previous_winners:[],history:[] };
const chip = { username:'ExampleMaker', display_name:'Example Maker', avatar:null, linked:true, deleted:false, faction:'glint' };
let choice = null, post = null, vote = null;
const privateReads=[];
const server=http.createServer(async(req,res)=>{
  const url=new URL(req.url,'http://127.0.0.1');
  const member=req.headers.cookie?.includes('member');
  const signed=!!req.headers.cookie;
  const restricted=req.headers.cookie?.includes('restricted');
  const faction=member?'aetheron':restricted?null:choice;
  let input=''; for await(const chunk of req) input+=chunk;
  const body=input?JSON.parse(input):{};
  let data={},status=200;
  if(url.pathname==='/api/auth/me') { if(signed) data={username:'ExampleViewer',faction,email_verified:true,providers:[],has_password:true}; else status=401; }
  else if(url.pathname==='/api/auth/sessions') data={sessions:[]};
  else if(url.pathname==='/api/auth/config') data={providers:[],turnstile_site_key:'',development:true};
  else if(url.pathname==='/api/me/alerts') data={};
  else if(url.pathname==='/api/me/following') data={items:[],next_cursor:null};
  else if(url.pathname==='/api/me/faction') { if(req.method==='PUT') choice=body.faction; data={faction:choice,can_choose:true,free_switch_available:!!choice,free_switch_until:'2026-10-12T00:00:00Z',between_seasons:false,next_switch_at:war.season.ends_at}; }
  else if(url.pathname==='/api/factions/war') data=war;
  else if(/^\/api\/factions\/\w+$/.test(url.pathname)) data={...war,is_member:member && url.pathname.endsWith('aetheron'),live:[],weekly_leaders:[{user:chip,influence:100}],season_leaders:[{user:chip,influence:200}]};
  else if(url.pathname.endsWith('/members')) data={items:[{user:chip,joined_at:war.season.starts_at}],next_cursor:null};
  else if(/\/(council|board|election|candidate)$/.test(url.pathname)) {
    privateReads.push({member,path:url.pathname});
    if(!member) {status=403; data={error:'Members only'};}
    else if(url.pathname.endsWith('/council')) {if(req.method==='PUT') vote=body.genre;data={my_vote:vote,target:'art',votes:[{genre:'art',votes:2}],closes_at:war.week.ends_at};}
    else if(url.pathname.endsWith('/board')) {if(req.method==='POST') post=body;data={items:post?[{...post,author:chip,created_at:war.season.starts_at,can_delete:false,can_report:true}]:[],next_cursor:null,can_post:true,slow_seconds:30};}
    else if(url.pathname.endsWith('/election')) data={candidate:false,my_vote:null,candidates:[{user:chip,votes:3}],closes_at:war.week.ends_at};
  }
  else if(url.pathname==='/api/streams') data={live:[],recent:[],has_more:false,as_of:war.season.starts_at};
  else if(url.pathname==='/api/categories') data={categories:[{id:'art',name:'Art',genre:'art'}]};
  else if(url.pathname.endsWith('/card')) data={...chip};
  else if(url.pathname.endsWith('/resolve')) {data={username:'ExampleMaker'};if(restricted)status=404;}
  else if(url.pathname.endsWith('/live')) data={live:false};
  else if(url.pathname.endsWith('/activity')) data={items:[],next_cursor:null};
  else if(url.pathname==='/api/channels/ExampleMaker') data={channel:{...chip,banner:null,bio:'Synthetic channel',mood_emoji:'',status_text:'',follower_count:2,following_count:3,links:[],song:null,live:false,joined_at:war.season.starts_at,season_rewards:[{season:1,faction:'glint'}]},viewer:{signed_in:signed,is_owner:false,following:false,blocked:false,interaction_blocked:false},tabs:{},header:{},war_council:{members:[],unavailable_count:0},wall_preview:{pinned:[],latest:[],viewer:{}},schedule_next:{items:[]}};
  else {status=404;data={error:'Not found'};}
  res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(data));
});
(async()=>{
  let next,browser,debugPage;
  try {
    await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(8080,'127.0.0.1',resolve);});
    let output='';
    next=spawn(process.execPath,[webRequire.resolve('next/dist/bin/next'),'start','--hostname','127.0.0.1','--port','13001'],{cwd:path.resolve('apps/web'),env:{...process.env,API_INTERNAL_ORIGIN:'http://127.0.0.1:8080'},stdio:['ignore','pipe','pipe'],windowsHide:true});
    next.stdout.on('data',c=>output+=c);next.stderr.on('data',c=>output+=c);
    for(let i=0;i<80&&!output.includes('Ready in');i++) {assert.equal(next.exitCode,null,output);await new Promise(r=>setTimeout(r,250));}
    assert.match(output,/Ready in/);
    browser=await chromium.launch({headless:true,channel:'msedge'});
    for(const width of [1440,1024,390,320]) {
      const context=await browser.newContext({viewport:{width,height:900}}),page=await context.newPage(),errors=[];
      debugPage=page;page.on('pageerror',e=>{errors.push(e.message);console.error('Browser error:',e.message);});
      for(const route of ['/','/war-map','/factions/aetheron']) {
        await page.goto('http://127.0.0.1:13001'+route);
        assert.equal(await page.locator('html').getAttribute('data-theme'),'neutral');
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,`${width} ${route} overflow`);
        if(process.env.SVER_SCREENSHOTS && route==='/war-map' && [1440,390].includes(width)) await page.screenshot({path:`tmp/factions-map-${width}.png`,fullPage:true});
        if(route==='/war-map') {
          if(width>=960) {const hex=page.getByRole('button',{name:/Art, held by/});await hex.focus();await page.keyboard.press('Enter');assert.equal(await hex.getAttribute('aria-pressed'),'true');await page.getByRole('button',{name:'Genre board',exact:true}).click();}
          assert.equal(await page.locator('.genre-card:visible').count(),16);
        }
        if(route.includes('aetheron')) assert.equal(await page.getByRole('heading',{name:'Members’ quarters'}).count(),0);
      }
      await context.addCookies([{name:'sver_dev',value:'synthetic-member',url:'http://127.0.0.1:13001'}]);
      await page.goto('http://127.0.0.1:13001/factions/aetheron');
      await page.getByRole('heading',{name:'Community board',exact:true}).waitFor();
      if(process.env.SVER_SCREENSHOTS && width===1440) await page.screenshot({path:'tmp/factions-hub-1440.png',fullPage:true});
      assert.equal(await page.locator('html').getAttribute('data-theme'),'aetheron');
      await page.getByRole('combobox',{name:/Target genre/}).selectOption('art');await page.getByRole('button',{name:vote?'Change vote':'Vote',exact:true}).click();
      await page.getByLabel('Message to your faction').fill('A safe <b>plain text</b> message');await page.getByRole('button',{name:'Post',exact:true}).click();
      await page.locator('.post-body').filter({hasText:'A safe <b>plain text</b> message'}).waitFor();assert.equal(await page.locator('.post-body b').count(),0);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,`${width} member hub overflow`);
      await page.goto('http://127.0.0.1:13001/ExampleMaker');
      assert.equal(await page.locator('.channel').getAttribute('data-theme'),'glint');assert.equal(await page.locator('html').getAttribute('data-theme'),'aetheron');
      await page.getByText('Season 1 champion',{exact:true}).waitFor();
      assert.deepEqual(errors,[]);await context.close();console.log(`${width}px: public/private hub, map, safe posts, voting, theme isolation, no overflow`);
    }
    const context=await browser.newContext({viewport:{width:390,height:844}}),page=await context.newPage();
    debugPage=page;
    await context.addCookies([{name:'sver_dev',value:'synthetic-new',url:'http://127.0.0.1:13001'}]);
    await page.goto('http://127.0.0.1:13001/account');await page.waitForURL('**/choose-faction');
    await page.locator('input[value="aetheron"]').check();assert.equal(await page.locator('html').getAttribute('data-theme'),'aetheron');
    await page.getByRole('button',{name:'Enlist in Aetheron'}).click();await page.getByRole('heading',{name:'Welcome to Aetheron'}).waitFor();
    if(process.env.SVER_SCREENSHOTS) await page.screenshot({path:'tmp/factions-welcome-390.png',fullPage:true});
    assert.equal(choice,'aetheron');assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
    assert(!privateReads.some(r=>!r.member),'Visitors never request private faction payloads');
    await context.addCookies([{name:'sver_dev',value:'synthetic-restricted',url:'http://127.0.0.1:13001'}]);
    await page.goto('http://127.0.0.1:13001/account');
    await page.getByRole('heading',{name:'Account security',exact:true}).waitFor();assert(page.url().endsWith('/account'));
    await context.close();console.log('Enrollment: required choice, theme preview, persisted choice, welcome, restricted-account security access; visitor privacy passed.');
  } catch(error) { if(debugPage&&!debugPage.isClosed()) console.error((await debugPage.locator('body').innerText({timeout:1000}).catch(()=>'' )).slice(-3000)); throw error; } finally {if(browser)await browser.close();if(next)next.kill();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
})().catch(e=>{console.error(e);process.exitCode=1;});
