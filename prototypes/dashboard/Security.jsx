(()=>{
const {StatGroup,Stat,DataTable,Tabs,Button,Badge,Modal,TextField,RadioCard,Icon,IconButton,LegendItem}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const TARGETS=[{name:'srv-eu-01',city:'Paris',ll:[2.35,48.86]},{name:'srv-eu-02',city:'Francfort',ll:[8.68,50.11]},{name:'srv-us-01',city:'Virginie',ll:[-77.49,39.04]},{name:'srv-ap-01',city:'Singapour',ll:[103.82,1.35]}];
const ORIGINS=[['RU','Moscou',[37.62,55.75]],['CN','Pékin',[116.4,39.9]],['CN','Shenzhen',[114.06,22.54]],['BR','São Paulo',[-46.63,-23.55]],['NG','Lagos',[3.38,6.52]],['VN','Hanoï',[105.83,21.03]],['IR','Téhéran',[51.39,35.69]],['RO','Bucarest',[26.1,44.43]],['IN','Mumbai',[72.88,19.08]],['ID','Jakarta',[106.85,-6.2]],['US','Dallas',[-96.8,32.78]],['KP','Pyongyang',[125.76,39.04]],['UA','Kharkiv',[36.23,49.99]],['TR','Istanbul',[28.98,41.01]],['AR','Buenos Aires',[-58.38,-34.6]],['ZA','Johannesburg',[28.05,-26.2]]];
const TYPES=[['Brute force SSH','22/tcp'],['Injection SQL','HTTPS'],['DDoS L7','HTTPS'],['Scan de ports','multi'],['Credential stuffing','/login'],['Path traversal','HTTPS'],['Bot malveillant','HTTPS']];
const ACTIONS=['Bloquée (WAF)','Bloquée (WAF)','Limitée','Bannie auto','Bloquée (pare-feu)'];
const rnd=a=>a[Math.floor(Math.random()*a.length)];
const ipOf=()=>[Math.floor(Math.random()*200)+20,Math.floor(Math.random()*255),Math.floor(Math.random()*255),Math.floor(Math.random()*254)+1].join('.');
const now=()=>new Date().toLocaleTimeString('fr-FR');
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
let seq=0;
const IP_POOL=Array.from({length:22},()=>{const o=rnd(ORIGINS);return {ip:ipOf(),cc:o[0],city:o[1],ll:o[2]}});
function makeEvent(rules){
  const src=Math.random()<.7?rnd(IP_POOL):(()=>{const o=rnd(ORIGINS);return {ip:ipOf(),cc:o[0],city:o[1],ll:o[2]}})();
  const t=rnd(TYPES),tg=rnd(TARGETS);
  const r=rules.find(x=>x.ip===src.ip);
  const action=r?(r.kind==='ban'?'Rejetée (IP bannie)':'Rejetée (IP bloquée)'):rnd(ACTIONS);
  return {id:++seq,time:now(),...src,type:t[0],port:t[1],target:tg,action,ruled:!!r};
}
const INITIAL_RULES=[
  {ip:'185.220.101.34',cc:'DE',kind:'block',reason:'Nœud de sortie Tor',expires:'Jamais',by:'Alex Martin'},
  {ip:'45.155.205.0/24',cc:'RU',kind:'block',reason:'Brute force SSH répété',expires:'Jamais',by:'Règle auto'},
  {ip:'103.152.118.72',cc:'VN',kind:'ban',reason:'Injection SQL',expires:'dans 23 h',by:'Règle auto'},
  {ip:'179.43.150.11',cc:'BR',kind:'ban',reason:'Credential stuffing',expires:'dans 4 h',by:'Lina Moreau'},
  {ip:'91.92.249.8',cc:'RO',kind:'ban',reason:'Scan de ports',expires:'dans 47 min',by:'Règle auto'},
  {ip:'82.65.14.201',cc:'FR',kind:'allow',reason:'Bureau Paris',expires:'Jamais',by:'Alex Martin'},
];

function AttackMap({events,paused}){
  const ref=React.useRef(null),api=React.useRef(null);
  React.useEffect(()=>{
    let alive=true;const el=ref.current;
    fetch('https://cdn.jsdelivr.net/npm/world-atlas@2.0.2/countries-110m.json').then(r=>r.json()).then(topo=>{
      if(!alive)return;
      const W=el.clientWidth,H=el.clientHeight;
      const land=topojson.feature(topo,topo.objects.countries);
      land.features=land.features.filter(f=>f.id!=='010');
      const proj=d3.geoNaturalEarth1().fitExtent([[10,10],[W-10,H-10]],land);
      const path=d3.geoPath(proj);
      const svg=d3.select(el).append('svg').attr('width',W).attr('height',H).style('display','block');
      const defs=svg.append('defs');
      const g=defs.append('linearGradient').attr('id','atk-grad').attr('gradientUnits','userSpaceOnUse');
      svg.append('path').datum(d3.geoGraticule10()).attr('d',path).attr('fill','none').attr('stroke','#141414').attr('stroke-width',.6);
      svg.append('g').selectAll('path').data(land.features).join('path').attr('d',path).attr('fill','#161616').attr('stroke','#242424').attr('stroke-width',.5);
      const arcs=svg.append('g'),fx=svg.append('g'),tg=svg.append('g');
      TARGETS.forEach(t=>{const [x,y]=proj(t.ll);const n=tg.append('g').attr('transform',`translate(${x},${y})`);
        n.append('circle').attr('r',10).attr('fill','none').attr('stroke','var(--text-strong)').attr('stroke-opacity',.25).attr('class','tgt-pulse');
        n.append('circle').attr('r',3.5).attr('fill','var(--text-strong)');
        n.append('text').text(t.city).attr('x',8).attr('y',-8).attr('fill','#a3a3a3').attr('font-size',10).attr('font-family','Geist, sans-serif');
      });
      api.current={fire(ev){
        const a=ev.ll,b=ev.target.ll,ip=d3.geoInterpolate(a,b);
        const pts=d3.range(0,1.0001,1/48).map(t=>proj(ip(t)));
        // lift arc for readability
        const [x0,y0]=pts[0],[x1,y1]=pts[pts.length-1],dist=Math.hypot(x1-x0,y1-y0);
        const lifted=pts.map((p,i)=>{const t=i/(pts.length-1);return [p[0],p[1]-Math.sin(Math.PI*t)*dist*.18]});
        const d=d3.line().curve(d3.curveBasis)(lifted);
        const col=ev.ruled?'#6f6f6f':'#ef4444';
        fx.append('circle').attr('cx',x0).attr('cy',y0).attr('r',2).attr('fill',col).transition().duration(1800).attr('r',9).attr('opacity',0).remove();
        fx.append('circle').attr('cx',x0).attr('cy',y0).attr('r',2.2).attr('fill',col).transition().delay(2600).duration(600).attr('opacity',0).remove();
        const p=arcs.append('path').attr('d',d).attr('fill','none').attr('stroke',col).attr('stroke-width',1.4).attr('stroke-linecap','round');
        const L=p.node().getTotalLength();
        p.attr('stroke-dasharray',`${L*.35} ${L}`).attr('stroke-dashoffset',L*.35)
          .transition().duration(1300).ease(d3.easeCubicInOut).attr('stroke-dashoffset',-L*.65)
          .transition().duration(250).attr('opacity',0).remove();
        const ghost=arcs.append('path').attr('d',d).attr('fill','none').attr('stroke',col).attr('stroke-opacity',.12).attr('stroke-width',1);
        ghost.transition().delay(1300).duration(1600).attr('stroke-opacity',0).remove();
        setTimeout(()=>{if(!alive)return;
          fx.append('circle').attr('cx',x1).attr('cy',y1).attr('r',3).attr('fill','none').attr('stroke',col).attr('stroke-width',1.2).transition().duration(900).ease(d3.easeCubicOut).attr('r',22).attr('opacity',0).remove();
        },1250);
      }};
    }).catch(()=>{});
    return ()=>{alive=false;api.current=null;d3.select(el).selectAll('*').remove()};
  },[]);
  const last=events[0]&&events[0].id;
  React.useEffect(()=>{if(events[0]&&api.current&&!paused)api.current.fire(events[0])},[last]);
  return <div ref={ref} style={{position:'absolute',inset:0}}></div>;
}

function FeedRow({ev,onBan,onBlock,rule}){
  const [h,setH]=React.useState(false);
  const act=(ic,t,f,col)=><button title={t} onClick={()=>f(ev)} style={{display:'flex',alignItems:'center',gap:5,height:24,padding:'0 8px',borderRadius:7,border:'1px solid rgba(255,255,255,.12)',background:'rgba(255,255,255,.04)',cursor:'pointer',font:'500 11px/1 var(--font-sans)',color:col}}><Icon name={ic} size={11} color={col}/>{t}</button>;
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} className="feed-in" style={{position:'relative',padding:'11px 16px',borderBottom:'1px solid rgba(255,255,255,.05)',display:'flex',flexDirection:'column',gap:7,background:h?'rgba(255,255,255,.025)':'transparent'}}>
    <div style={{display:'flex',alignItems:'center',gap:8,minHeight:24}}>
      <span style={{width:6,height:6,borderRadius:99,background:ev.ruled?'var(--viz-2)':'var(--red-500)',flex:'none'}}></span>
      <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)',flex:1,minWidth:0,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{ev.type}</span>
      {h&&!rule?<div style={{display:'flex',gap:4,flex:'none'}}>{act('clock','Bannir',onBan,'var(--text-body)')}{act('ban','Bloquer',onBlock,'#f87171')}</div>:mono(ev.time,'var(--text-faint)')}
    </div>
    <div style={{display:'flex',alignItems:'center',gap:7,font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)',minWidth:0}}>
      {mono(ev.ip,'var(--text-body)')}<span style={{color:'var(--text-faint)',flex:'none'}}>{ev.cc}</span>
      <Icon name="arrow-right" size={11} color="var(--text-faint)"/><span style={{whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis',minWidth:0}}>{ev.target.name}</span>
      <span style={{flex:1}}></span>
      {rule?<span style={{flex:'none',height:20,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:6,font:'500 11px/1 var(--font-sans)',background:rule.kind==='allow'?'rgba(134,239,172,.12)':'rgba(255,255,255,.07)',color:rule.kind==='allow'?'#86efac':'var(--text-body)'}}>{rule.kind==='ban'?'Bannie':rule.kind==='block'?'Bloquée':'Autorisée'}</span>
        :<span style={{flex:'none',maxWidth:110,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis',color:'var(--text-subtle)'}}>{ev.action}</span>}
    </div>
  </div>;
}

function BanModal({open,initial,onClose,onSubmit}){
  const [ip,setIp]=React.useState('');const [kind,setKind]=React.useState('ban');const [dur,setDur]=React.useState('24 h');const [reason,setReason]=React.useState('');
  React.useEffect(()=>{if(open){setIp(initial?.ip||'');setKind(initial?.kind||'ban');setReason(initial?.reason||'');setDur('24 h')}},[open]);
  if(!open)return null;
  const DUR=['1 h','24 h','7 jours','30 jours'];
  return <Modal open title="Ajouter une règle IP" description="S'applique immédiatement sur tous les agents via le pare-feu nftables et le WAF Caddy." onClose={onClose} width={520}
    footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!ip} onClick={()=>onSubmit({ip,kind,reason:reason||'Manuel',expires:kind==='ban'?'dans '+dur:'Jamais'})}>{kind==='ban'?'Bannir':kind==='block'?'Bloquer':'Autoriser'}</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:18}}>
      <TextField label="Adresse IP ou plage CIDR" mono placeholder="203.0.113.0/24" value={ip} onChange={setIp}/>
      <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:8}}>
        <RadioCard icon="timer" selected={kind==='ban'} onClick={()=>setKind('ban')} title="Bannir" description="Temporaire"/>
        <RadioCard icon="ban" selected={kind==='block'} onClick={()=>setKind('block')} title="Bloquer" description="Permanent"/>
        <RadioCard icon="shield-check" selected={kind==='allow'} onClick={()=>setKind('allow')} title="Autoriser" description="Liste blanche"/>
      </div>
      {kind==='ban'&&<div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Durée</span>
        <div style={{display:'flex',gap:6}}>{DUR.map(d=><Button key={d} size="sm" variant={dur===d?'inverse':'secondary'} pill onClick={()=>setDur(d)}>{d}</Button>)}</div></div>}
      <TextField label="Raison" placeholder="Brute force SSH" value={reason} onChange={setReason}/>
    </div>
  </Modal>;
}

function useCrowdsec(){const read=()=>{let on=null,cfg=null;try{on=JSON.parse(localStorage.getItem('ciphera.connectors.on'));cfg=JSON.parse(localStorage.getItem('ciphera.crowdsec'))}catch(e){}const connected=Array.isArray(on)?on.includes('crowdsec'):true;return {connected,plan:(cfg&&cfg.plan)||'free',share:!!(cfg&&cfg.share)}};const [v,setV]=React.useState(read);React.useEffect(()=>{const f=()=>setV(read());window.addEventListener('ciphera:connectors',f);window.addEventListener('storage',f);return()=>{window.removeEventListener('ciphera:connectors',f);window.removeEventListener('storage',f)}},[]);return v}
function SecurityPage({banOpen,closeBan,onNav}){
  const cs=useCrowdsec();
  const [rules,setRules]=React.useState(INITIAL_RULES);
  const [events,setEvents]=React.useState(()=>Array.from({length:7},()=>makeEvent(INITIAL_RULES)));
  const [paused,setPaused]=React.useState(false);
  const [blocked,setBlocked]=React.useState(48213);
  const [tab,setTab]=React.useState('ban');
  const [modal,setModal]=React.useState(null);
  const rulesRef=React.useRef(rules);rulesRef.current=rules;
  React.useEffect(()=>{if(paused)return;const t=setInterval(()=>{const ev=makeEvent(rulesRef.current);setEvents(e=>[ev,...e].slice(0,40));setBlocked(b=>b+1)},1100);return()=>clearInterval(t)},[paused]);
  React.useEffect(()=>{if(banOpen)setModal({})},[banOpen]);
  const ruleFor=ip=>rules.find(r=>r.ip===ip);
  const addRule=r=>{setRules(rs=>[{cc:events.find(e=>e.ip===r.ip)?.cc||'—',by:'Alex Martin',...r},...rs.filter(x=>x.ip!==r.ip)]);setTab(r.kind);setModal(null);closeBan&&closeBan()};
  const quick=(ev,kind)=>addRule({ip:ev.ip,kind,reason:ev.type,expires:kind==='ban'?'dans 24 h':'Jamais',cc:ev.cc});
  const remove=ip=>setRules(rs=>rs.filter(r=>r.ip!==ip));
  const convert=ip=>setRules(rs=>rs.map(r=>r.ip===ip?{...r,kind:'block',expires:'Jamais'}:r));
  const list=rules.filter(r=>r.kind===tab);
  const count=k=>rules.filter(r=>r.kind===k).length;
  const live=events.filter(e=>!e.ruled).length;
  const byType=TYPES.map(t=>[t[0],events.filter(e=>e.type===t[0]).length]).sort((a,b)=>b[1]-a[1]).slice(0,4);
  const removeLabel={ban:'Débannir',block:'Débloquer',allow:'Retirer'}[tab];
  const P='139,124,246';const box={borderRadius:16,border:'1px solid rgba(255,255,255,.08)',background:'#0c0c0f'};
  const [auto,setAuto]=React.useState(true);const [menu,setMenu]=React.useState(false);const [q,setQ]=React.useState('');
  React.useEffect(()=>{if(!menu)return;const f=()=>setMenu(false);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  const spk=(d,col)=>{const m=Math.max(...d),w=88,h=34,pts=d.map((v,i)=>[i/(d.length-1)*w,h-v/m*(h-4)-2]);const p=pts.map((x,i)=>(i?'L':'M')+x[0].toFixed(1)+','+x[1].toFixed(1)).join(' ');return <svg width={w} height={h} style={{flex:'none'}}><path d={p+' L'+w+','+h+' L0,'+h+' Z'} fill={'rgba('+col+',.18)'}/><path d={p} fill="none" stroke={'rgb('+col+')'} strokeWidth="1.5"/></svg>};
  const STATS=[['Attaques bloquées (24 h)',blocked.toLocaleString('fr-FR'),'−18,2 % vs veille',[8,9,7,11,10,14,12,15,13,17,16,19],'248,113,113'],['IP bannies',String(count('ban')),'expiration auto 24 h',[2,3,3,4,3,5,4,6,5,6,7,6],P],['IP bloquées',String(count('block')),'permanent',[1,1,2,2,2,3,3,3,4,4,4,5],'125,211,252'],['Règles WAF actives','142','+6 cette semaine',[120,122,125,126,128,130,131,134,136,138,140,142],'134,239,172']];
  if(!cs.connected)return <div style={{padding:'20px 32px 40px 34px'}}><div style={{...box,display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',gap:14,padding:'56px 24px'}}><span style={{width:64,height:64,borderRadius:18,display:'flex',alignItems:'center',justifyContent:'center',background:'#fff'}}><img src="../../web/src/assets/images/connectors/crowdsec-mark.png" alt="" style={{width:44,height:44,objectFit:'contain'}}/></span><span style={{font:'600 17px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Connectez CrowdSec pour activer la sécurité</span><span style={{maxWidth:440,font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)'}}>La détection d'attaques, le bannissement d'IP et les blocklists passent par le connecteur CrowdSec. Il n'est pas connecté pour le moment.</span><button onClick={()=>onNav&&onNav('Connecteurs')} style={{display:'flex',alignItems:'center',gap:8,height:38,padding:'0 16px',borderRadius:11,border:0,cursor:'pointer',background:'rgb('+P+')',color:'#fff',font:'600 13px/1 var(--font-sans)'}}><Icon name="plug" size={14} color="#fff"/>Connecter CrowdSec</button></div></div>;
  const rows=list.filter(r=>(r.ip+r.reason+(r.cc||'')).toLowerCase().includes(q.toLowerCase()));
  const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};const hl='1px solid var(--border-subtle)';
  const Sp=({d})=>{const w=112,h=44,m=Math.max(...d),n=Math.min(...d);const pts=d.map((v,i)=>[i/(d.length-1)*w,h-4-(v-n)/((m-n)||1)*(h-10)]);const pa=pts.map((q2,i)=>(i?'L':'M')+q2[0].toFixed(1)+','+q2[1].toFixed(1)).join(' ');const e=pts[pts.length-1];return <svg width={w} height={h} style={{flex:'none',overflow:'visible'}}><path d={pa+' L'+w+','+h+' L0,'+h+' Z'} fill="rgba(139,124,246,.18)"/><path d={pa} fill="none" stroke="#8b7cf6" strokeWidth="1.5"/><circle cx={e[0]} cy={e[1]} r="3.5" fill="#a78bfa"/></svg>};
  const Tile=({title,val,unit,d})=><div style={{display:'flex',flexDirection:'column',gap:14,padding:'18px 20px',borderRadius:14,border:'1px solid var(--border-default)'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><span style={{display:'flex',alignItems:'flex-end',gap:12}}><span style={{display:'flex',flexDirection:'column',gap:9,flex:1}}><span style={{display:'flex',alignItems:'baseline',gap:5}}><span style={{font:'600 24px/1 var(--font-sans)',color:'var(--text-strong)'}}>{val}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{unit}</span></span><span style={{...lbl,fontSize:11}}>Dernières 24 heures</span></span><Sp d={d}/></span></div>;
  const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;
  return <div style={{padding:'20px 32px 48px 34px',display:'flex',flexDirection:'column',gap:14}}>
    <div style={{...card,borderRadius:20,padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',alignItems:'center',gap:16}}>
        <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#fff'}}><img src="../../web/src/assets/images/connectors/crowdsec-mark.png" alt="CrowdSec" style={{width:36,height:36,objectFit:'contain'}}/></span>
        <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>CrowdSec</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:'rgba(34,197,94,.12)',font:'500 12px/1 var(--font-sans)',color:'var(--green-500)'}}>Connecté</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:cs.plan==='pro'?'rgba(139,124,246,.15)':'var(--surface-selected)',font:'600 11px/1 var(--font-sans)',color:cs.plan==='pro'?'#c4b5fd':'var(--text-body)'}}>{cs.plan==='pro'?'Pro':'Gratuit'}</span></span><span style={lbl}>4 agents · {cs.plan==='pro'?'blocklists premium':'blocklists communautaires'}</span></span>
        <div style={{display:'flex',borderRadius:10,overflow:'hidden'}}><button onClick={()=>setModal({kind:tab})} style={{display:'flex',alignItems:'center',gap:7,height:38,padding:'0 16px',border:0,cursor:'pointer',background:'#8b7cf6',color:'#fff',font:'600 13px/1 var(--font-sans)'}}><Icon name="plus" size={14} color="#fff"/>Ajouter une règle</button><button onClick={()=>setModal({kind:'block'})} title="Bloquer une IP" style={{width:38,height:38,border:0,borderLeft:'1px solid rgba(0,0,0,.2)',cursor:'pointer',background:'#8b7cf6',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="ban" size={14} color="#fff"/></button></div>
        <Button iconRight="arrow-up-right" onClick={()=>onNav&&onNav('Connecteurs')}>Console CrowdSec</Button>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
        <span>Bannissement auto : <span style={{fontWeight:600,color:auto?'var(--green-500)':'var(--text-strong)'}}>{auto?'activé':'désactivé'}</span></span>{sep}
        <span>Partage communautaire : <span style={{fontWeight:600,color:'var(--text-strong)'}}>{cs.share?'activé':'désactivé'}</span></span>{sep}
        <span>Flux : <span style={{color:'var(--text-strong)'}}>{paused?'en pause':'en direct'}</span></span>
        <span style={{flex:1}}></span>
        <button onClick={()=>setAuto(v=>!v)} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'#a78bfa'}}>{auto?'Désactiver le bannissement auto':'Activer le bannissement auto'}</button>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:12}}>
        <Tile title="Attaques bloquées" val={blocked.toLocaleString('fr-FR')} unit="" d={[8,9,7,11,10,14,12,15,13,17,16,19,14,18,21]}/>
        <Tile title="IP bannies" val={count('ban')} unit="actives" d={[2,3,3,4,3,5,4,6,5,6,7,6,7,8,count('ban')]}/>
        <Tile title="IP bloquées" val={count('block')} unit="permanentes" d={[1,1,2,2,2,3,3,3,4,4,4,5,5,5,count('block')]}/>
        <Tile title="Règles WAF" val="142" unit="actives" d={[120,122,125,126,128,130,131,134,136,138,140,141,142,142,142]}/>
      </div>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) 340px',gap:14}}>
      <div style={{...card,borderRadius:18,display:'flex',flexDirection:'column',overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',gap:12,padding:'16px 20px'}}>
          <span className={paused?'':'live-dot'} style={{width:8,height:8,borderRadius:99,background:paused?'var(--viz-2)':'var(--red-500)'}}></span>
          <span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>Attaques en direct</span>
          <span style={{flex:1}}></span>
          <div style={{display:'flex',gap:16,alignItems:'center'}}><LegendItem color="var(--red-500)" label="Origine" underline={false}/><LegendItem color="var(--gray-950)" label="Serveur ciblé" underline={false}/></div>
          <Button size="sm" icon={paused?'play':'pause'} onClick={()=>setPaused(p=>!p)}>{paused?'Reprendre':'Pause'}</Button>
        </div>
        <div style={{position:'relative',height:400,margin:'0 10px'}}><AttackMap events={events} paused={paused}/></div>
        <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:22,margin:'0 16px 16px',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)'}}>
          {byType.map(([t,n])=><div key={t} style={{display:'flex',flexDirection:'column',gap:10,minWidth:0}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{t}</span><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{flex:1,height:5,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:(n/Math.max(1,events.length)*100)+'%',borderRadius:99,background:'#ededed'}}></span></span>{mono(String(n),'var(--text-strong)')}</span></div>)}
        </div>
      </div>
      <div style={{...card,borderRadius:18,display:'flex',flexDirection:'column',overflow:'hidden',maxHeight:540}}>
        <div style={{display:'flex',alignItems:'center',gap:10,padding:'16px 18px'}}><span style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>Flux d'événements</span><span style={{...lbl,fontSize:11}}>{live} non filtrées sur {events.length}</span></span><Badge>{live}</Badge></div>
        <div className="ds-scroll-hidden" style={{overflowY:'auto',flex:1,scrollbarWidth:'none',borderTop:hl}}>
          {events.map(ev=><FeedRow key={ev.id} ev={ev} rule={ruleFor(ev.ip)} onBan={e=>quick(e,'ban')} onBlock={e=>quick(e,'block')}/>)}
        </div>
      </div>
    </div>
    <div style={{paddingLeft:12,marginTop:6}}><Tabs tabs={[{value:'ban',label:'IP bannies',count:count('ban')},{value:'block',label:'IP bloquées',count:count('block')},{value:'allow',label:'Liste blanche',count:count('allow')}]} value={tab} onChange={setTab}/></div>
    <div style={{...card,overflow:'visible'}}>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.3fr) 70px minmax(0,1.4fr) minmax(0,1fr) 120px 180px',gap:16,alignItems:'center',padding:'14px 16px'}}>
        {['Adresse IP','Pays','Raison','Par','Expiration'].map(h2=><span key={h2} style={{display:'flex',alignItems:'center',gap:6,font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{h2}<Icon name="arrow-down-up" size={12}/></span>)}
        <span style={{display:'flex',alignItems:'center',gap:8,height:30,padding:'0 10px',borderRadius:8,border:'1px solid var(--border-default)'}}><Icon name="search" size={13} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Filtrer…" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 12px/1 var(--font-sans)'}}/></span>
      </div>
      {rows.map(r=><div key={r.ip} style={{display:'grid',gridTemplateColumns:'minmax(0,1.3fr) 70px minmax(0,1.4fr) minmax(0,1fr) 120px 180px',gap:16,alignItems:'center',padding:'14px 16px',borderTop:hl}}>
        <span style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}><span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={tab==='allow'?'shield-check':tab==='block'?'ban':'clock'} size={17} color={tab==='allow'?'var(--green-500)':tab==='block'?'#f87171':'var(--text-strong)'}/></span><span style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}>{mono(r.ip,'var(--text-strong)',13)}<span style={{...lbl,fontSize:11}}>{tab==='ban'?'Bannie':tab==='block'?'Bloquée':'Autorisée'}</span></span></span>
        <span style={{display:'inline-flex',width:'fit-content',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:'var(--text-body)'}}>{r.cc||'—'}</span>
        <span style={{font:'400 13px/1.3 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{r.reason}</span>
        <span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{r.by}</span>
        <span><span style={{display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,font:'500 12px/1 var(--font-mono)',background:r.expires==='Jamais'?'var(--surface-selected)':'rgba(229,180,84,.12)',color:r.expires==='Jamais'?'var(--text-body)':'#e5b454'}}>{r.expires}</span></span>
        <div style={{display:'flex',gap:6,justifyContent:'flex-end'}}>{r.kind==='ban'&&<Button size="sm" variant="ghost" onClick={()=>convert(r.ip)}>Bloquer</Button>}<Button size="sm" onClick={()=>remove(r.ip)}>{removeLabel}</Button></div>
      </div>)}
      {!rows.length&&<div style={{padding:'28px 16px',borderTop:hl,...lbl}}>Aucune règle.</div>}
    </div>
    <BanModal open={!!modal} initial={modal} onClose={()=>{setModal(null);closeBan&&closeBan()}} onSubmit={addRule}/>
  </div>;
}
window.SecurityPage=SecurityPage;
})();
