(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Select,SegmentedBar,LegendItem}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const card={borderRadius:16,border:'1px solid rgba(255,255,255,.08)',background:'#0c0c0f'};
const RANGES={'1 h':[60,'min',1],'24 h':[48,'h',.5],'7 j':[56,'j',1/8],'30 j':[60,'j',.5]};
const SERIES_COL=['var(--text-strong)','#8a8a8a','#5a5a5a','#3f3f3f'];
const num=s=>parseFloat(String(s).replace(',','.'))||0;
function wave(n,base,amp,seed,spike){return Array.from({length:n},(_,i)=>{const v=base+Math.sin((i+seed)/4.3)*amp*.5+Math.sin((i+seed*2)/1.7)*amp*.25+((i*7+seed*13)%11)/11*amp*.35+(spike&&i===spike?amp*1.6:0);return Math.max(0,Math.min(100,v))})}
function Bar({v,h=4}){return <span style={{display:'block',height:h,borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:Math.min(100,v)+'%',borderRadius:99,background:v>=85?'var(--red-500)':v>=70?'#e5b454':'var(--viz-1)',transition:'width .4s var(--ease-out)'}}></span></span>}
function Ticks({v,n=40}){const on=Math.round(v/100*n);return <div style={{display:'flex',gap:2,height:22}}>{Array.from({length:n},(_,i)=><span key={i} style={{flex:1,borderRadius:1,background:i<on?(v>=85?'var(--red-500)':v>=70?'#e5b454':'var(--viz-1)'):'var(--viz-4)'}}></span>)}</div>}

function MultiChart({series,labels,unit='%',yMax=100,height=220}){
  const ref=React.useRef(null);const [w,setW]=React.useState(700);const [idx,setIdx]=React.useState(null);
  React.useEffect(()=>{const ro=new ResizeObserver(e=>setW(e[0].contentRect.width));ro.observe(ref.current);return()=>ro.disconnect()},[]);
  const n=series[0].data.length,x=i=>i/(n-1)*w,y=v=>height-v/yMax*height;
  const path=d=>d.map((v,i)=>(i?'L':'M')+x(i).toFixed(1)+','+y(v).toFixed(1)).join(' ');
  const ticks=[0,.25,.5,.75,1].map(f=>Math.round(yMax*f));
  return <div style={{display:'grid',gridTemplateColumns:'40px minmax(0,1fr)',columnGap:8}}>
    <div style={{position:'relative',height}}>{ticks.map(t=><span key={t} style={{position:'absolute',right:0,top:y(t)-6,font:'400 11px/1 var(--font-mono)',color:'var(--text-faint)'}}>{t}{unit==='%'?'%':''}</span>)}</div>
    <div ref={ref} onMouseMove={e=>{const r=ref.current.getBoundingClientRect();setIdx(Math.max(0,Math.min(n-1,Math.round((e.clientX-r.left)/w*(n-1)))))}} onMouseLeave={()=>setIdx(null)} style={{position:'relative',height,cursor:'crosshair'}}>
      <svg width={w} height={height} style={{position:'absolute',inset:0,overflow:'visible'}}>
        <defs><linearGradient id="mon-a" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stopColor="#fff" stopOpacity=".09"/><stop offset="1" stopColor="#fff" stopOpacity="0"/></linearGradient></defs>
        {ticks.map(t=><line key={t} x1="0" x2={w} y1={y(t)} y2={y(t)} stroke="#1a1a1a" strokeDasharray="2 3"/>)}
        <path d={path(series[0].data)+` L${w},${height} L0,${height} Z`} fill="url(#mon-a)"/>
        {series.map((s,j)=><path key={s.name} d={path(s.data)} fill="none" stroke={SERIES_COL[j]} strokeWidth={j?1.2:1.6} strokeLinejoin="round"/>)}
        {idx!=null&&<><line x1={x(idx)} x2={x(idx)} y1="0" y2={height} stroke="#555"/>{series.map((s,j)=><circle key={j} cx={x(idx)} cy={y(s.data[idx])} r="3.5" fill={SERIES_COL[j]} stroke="#0b0b0b" strokeWidth="1.5"/>)}</>}
      </svg>
      {idx!=null&&<div style={{position:'absolute',top:0,left:x(idx)>w*.6?undefined:x(idx)+12,right:x(idx)>w*.6?w-x(idx)+12:undefined,minWidth:150,padding:'10px 12px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',pointerEvents:'none',display:'flex',flexDirection:'column',gap:8}}>
        <span style={{font:'500 11px/1 var(--font-sans)',color:'var(--text-strong)'}}>{labels[idx]}</span>
        {series.map((s,j)=><span key={j} style={{display:'flex',alignItems:'center',gap:8,font:'400 12px/1 var(--font-sans)'}}><span style={{width:8,height:2,background:SERIES_COL[j]}}></span><span style={{flex:1,color:'var(--text-muted)'}}>{s.name}</span><span style={{fontFamily:'var(--font-mono)',color:'var(--text-strong)'}}>{s.fmt?s.fmt(s.data[idx]):Math.round(s.data[idx])+'%'}</span></span>)}
      </div>}
    </div>
    <div></div>
    <div style={{display:'flex',justifyContent:'space-between',marginTop:10,font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}>{[0,.25,.5,.75,1].map(f=><span key={f}>{labels[Math.round(f*(n-1))]}</span>)}</div>
  </div>;
}

function MonitoringPage(){
  const D=window.CIPHERA;const SRV=D.servers;
  const [scope,setScope]=React.useState('Tous les serveurs');
  const [range,setRange]=React.useState('24 h');
  const [metric,setMetric]=React.useState('CPU');
  const [live,setLive]=React.useState(true);
  const [tick,setTick]=React.useState(0);
  React.useEffect(()=>{if(!live)return;const t=setInterval(()=>setTick(x=>x+1),2000);return()=>clearInterval(t)},[live]);
  const [N,u]=RANGES[range];
  const labels=Array.from({length:N},(_,i)=>{const k=N-1-i;return k===0?'maintenant':u==='min'?'−'+k+' min':u==='h'?'−'+Math.round(k/2)+' h':'−'+Math.round(k*RANGES[range][2]*(range==='7 j'?1:1))+' j'});
  const online=SRV.filter(s=>s.status[0]!=='error');
  const list=scope==='Tous les serveurs'?SRV:SRV.filter(s=>s.name===scope);
  const sel=list.filter(s=>s.status[0]!=='error');
  const jitter=v=>Math.max(0,Math.min(100,v+((tick*7+v)%5)-2));
  const cpuNow=sel.length?Math.round(sel.reduce((a,s)=>a+jitter(s.cpu),0)/sel.length):0;
  const ramUsed=sel.reduce((a,s)=>a+num(s.ram.split('/')[0]),0),ramTot=list.reduce((a,s)=>a+num(s.ram.split('/')[1]),0);
  const diskUsed=list.reduce((a,s)=>a+num(s.disk.split('/')[0]),0),diskTot=list.reduce((a,s)=>a+num(s.disk.split('/')[1]),0);
  const netIn=(sel.length*38.4+tick%7).toFixed(1),netOut=(sel.length*12.1+tick%3).toFixed(1);
  const ramPct=ramTot?Math.round(ramUsed/ramTot*100):0,diskPct=diskTot?Math.round(diskUsed/diskTot*100):0;
  const seed=tick+(scope.length);
  const chartSeries={
    'CPU':sel.slice(0,4).map((s,i)=>({name:s.name,data:wave(N,s.cpu,18,seed+i*5,i===1?Math.floor(N*.7):0)})),
    'Mémoire':sel.slice(0,4).map((s,i)=>({name:s.name,data:wave(N,num(s.ram)/num(s.ram.split('/')[1])*100,6,seed+i*3)})),
    'Disque I/O':[{name:'Lecture',data:wave(N,34,30,seed),fmt:v=>(v*1.8).toFixed(0)+' Mo/s'},{name:'Écriture',data:wave(N,18,20,seed+9),fmt:v=>(v*1.8).toFixed(0)+' Mo/s'}],
    'Réseau':[{name:'Entrant',data:wave(N,48,30,seed+2),fmt:v=>(v*1.6).toFixed(1)+' Mb/s'},{name:'Sortant',data:wave(N,20,14,seed+6),fmt:v=>(v*1.6).toFixed(1)+' Mb/s'}],
  }[metric];
  const KPI=[
    ['cpu','CPU',cpuNow+' %',cpuNow,sel.reduce((a,s)=>a+(s.cores||2),0)+' vCPU · '+sel.length+' nœuds'],
    ['memory-stick','Mémoire',ramUsed.toFixed(1).replace('.',',')+' / '+ramTot+' Go',ramPct,ramPct+' % utilisés'],
    ['hard-drive','Stockage',diskUsed+' / '+diskTot+' Go',diskPct,(diskTot-diskUsed)+' Go libres'],
    ['arrow-down-up','Réseau','↓ '+netIn+'  ↑ '+netOut,Math.min(100,netIn/2),'Mb/s · entrant / sortant'],
  ];
  const TOP=[['web-1','ciphera-web',38,'236 Mo'],['worker-1','ciphera-web',31,'388 Mo'],['api-1','billing-api',27,'512 Mo'],['postgres-1','ciphera-web',19,'1,1 Go'],['clickhouse-1','analytics',16,'2,4 Go'],['web-1','dashboard-v2',9,'148 Mo']];
  const ALERTS=[['error',"Taux d'erreur > 2 % pendant 5 min",'billing-api','il y a 1 h'],['error','Agent injoignable','srv-iad-01','il y a 3 h'],['pending','CPU > 85 %','srv-par-02','il y a 6 h'],['pending','Certificat mTLS expire dans 3 j','srv-iad-01','il y a 1 j']];
  const segBtn=on=>({height:28,padding:'0 10px',border:0,borderRadius:7,cursor:'pointer',background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',font:'500 12px/1 var(--font-sans)'});
  const score=Math.round(100-(cpuNow*.3+ramPct*.25+diskPct*.15)/1.2);
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <div style={{position:'relative',display:'flex',alignItems:'center',gap:20,padding:'20px 24px',borderRadius:18,overflow:'hidden',border:'1px solid rgba(56,189,248,.3)',background:'linear-gradient(110deg,#0e7490,#0ea5e9 55%,#38bdf8)'}}><span style={{position:'absolute',right:-40,top:-70,width:240,height:240,borderRadius:99,background:'rgba(255,255,255,.12)'}}></span><span style={{position:'relative',width:56,height:56,borderRadius:16,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(255,255,255,.95)',font:'700 18px/1 var(--font-sans)',color:'#0e7490'}}>{score}</span><span style={{position:'relative',display:'flex',flexDirection:'column',gap:7,flex:1}}><span style={{font:'700 17px/1 var(--font-sans)',color:'#fff'}}>Santé de l'infrastructure</span><span style={{font:'400 12px/1 var(--font-sans)',color:'rgba(255,255,255,.88)'}}>{online.length} / {SRV.length} agents connectés · CPU {cpuNow} % · mémoire {ramPct} % · stockage {diskPct} %</span></span><span style={{position:'relative',display:'flex',alignItems:'center',gap:6,height:26,padding:'0 10px',borderRadius:99,background:'rgba(255,255,255,.88)',font:'600 11px/1 var(--font-sans)',color:'#0c4a6e'}}><span style={{width:6,height:6,borderRadius:99,background:live?'#16a34a':'#a1a1aa'}}></span>{live?'En direct':'En pause'}</span></div>
    <div style={{display:'flex',alignItems:'center',gap:10}}>
      <Select icon="server" options={['Tous les serveurs',...SRV.map(s=>s.name)]} value={scope} onChange={setScope}/>
      <div style={{display:'flex',gap:2,padding:2,borderRadius:9,border:'1px solid var(--border-default)',background:'var(--surface-control)'}}>{Object.keys(RANGES).map(r=><button key={r} onClick={()=>setRange(r)} style={segBtn(range===r)}>{r}</button>)}</div>
      <span style={{flex:1}}></span>
      <span style={lbl}>{online.length} / {SRV.length} agents connectés</span>
      <Button icon={live?'pause':'play'} onClick={()=>setLive(v=>!v)}><span style={{display:'flex',alignItems:'center',gap:8}}>{live&&<span className="live-ping" style={{width:6,height:6,borderRadius:99,background:'var(--green-500)'}}></span>}{live?'En direct':'En pause'}</span></Button>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:12}}>
      {KPI.map(([ic,k,v,p,sub])=><button key={k} onClick={()=>setMetric(k==='Stockage'?'Disque I/O':k)} style={{...card,padding:'16px 18px',display:'flex',flexDirection:'column',gap:14,textAlign:'left',cursor:'pointer',borderColor:(metric===k||(k==='Stockage'&&metric==='Disque I/O'))?'var(--gray-500)':'var(--border-default)'}}>
        <span style={{display:'flex',alignItems:'center',gap:8,...lbl}}><Icon name={ic} size={14} color="var(--text-muted)"/>{k}<span style={{flex:1}}></span>{k!=='Réseau'&&<span style={{fontFamily:'var(--font-mono)',fontSize:11,color:p>=85?'#f87171':p>=70?'#e5b454':'var(--text-subtle)'}}>{p} %</span>}</span>
        <span style={{font:'500 20px/1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em',whiteSpace:'pre'}}>{v}</span>
        <Ticks v={p}/>
        <span style={{...lbl,fontSize:11}}>{sub}</span>
      </button>)}
    </div>
    <div style={{...card,padding:'18px 20px 16px'}}>
      <div style={{display:'flex',alignItems:'center',gap:12,marginBottom:22}}>
        <div style={{display:'flex',gap:2,padding:2,borderRadius:9,border:'1px solid var(--border-default)',background:'var(--surface-control)'}}>{['CPU','Mémoire','Disque I/O','Réseau'].map(m=><button key={m} onClick={()=>setMetric(m)} style={segBtn(metric===m)}>{m}</button>)}</div>
        <span style={{flex:1}}></span>
        <div style={{display:'flex',gap:16,flexWrap:'wrap'}}>{chartSeries.map((s,j)=><span key={s.name} style={{display:'flex',alignItems:'center',gap:8,font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}><span style={{width:10,height:2,background:SERIES_COL[j]}}></span>{s.name}</span>)}</div>
      </div>
      <MultiChart series={chartSeries} labels={labels} unit={metric==='CPU'||metric==='Mémoire'?'%':''}/>
    </div>
    <div style={{...card,overflow:'hidden'}}>
      <div style={{display:'flex',alignItems:'center',padding:'16px 18px'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Serveurs</span></div>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.3fr) repeat(3,minmax(0,1fr)) 90px 80px',gap:20,padding:'10px 18px',borderTop:hair,...lbl}}><span>Nœud</span><span>CPU</span><span>Mémoire</span><span>Disque</span><span>Charge</span><span style={{textAlign:'right'}}>Conteneurs</span></div>
      {list.map(s=>{const off=s.status[0]==='error';const c=off?0:jitter(s.cpu),r=off?0:Math.round(num(s.ram)/num(s.ram.split('/')[1])*100),d=Math.round(num(s.disk)/num(s.disk.split('/')[1])*100);return <div key={s.name} style={{display:'grid',gridTemplateColumns:'minmax(0,1.3fr) repeat(3,minmax(0,1fr)) 90px 80px',gap:20,alignItems:'center',padding:'14px 18px',borderTop:hair,opacity:off?.55:1}}>
        <div style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:8}}><StatusDot status={s.status[0]}/>{mono(s.name,'var(--text-strong)')}</span><span style={{...lbl,fontSize:11,paddingLeft:15}}>{s.region}</span></div>
        {[[c,c+' %'],[r,s.ram],[d,s.disk]].map(([p,t],i)=><div key={i} style={{display:'flex',flexDirection:'column',gap:7}}><span style={{font:'400 12px/1 var(--font-mono)',color:off?'var(--text-faint)':'var(--text-body)'}}>{off?'—':t}</span><Bar v={p}/></div>)}
        <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-muted)'}}>{off?'—':(c/100*(s.cores||2)).toFixed(2)}</span>
        <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)',textAlign:'right'}}>{s.containers}</span>
      </div>})}
    </div>
    <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.2fr) minmax(0,1fr)',gap:12}}>
      <div style={{...card,overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'16px 18px'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Conteneurs les plus gourmands</span><span style={lbl}>CPU · mémoire</span></div>
        {TOP.map(([c,p,cpu,mem],i)=><div key={i} style={{display:'grid',gridTemplateColumns:'18px minmax(0,1fr) 120px 64px',gap:12,alignItems:'center',padding:'11px 18px',borderTop:hair}}>
          <span style={{...lbl,fontFamily:'var(--font-mono)',fontSize:11}}>{i+1}</span>
          <div style={{display:'flex',flexDirection:'column',gap:5,minWidth:0}}>{mono(c,'var(--text-strong)')}<span style={{...lbl,fontSize:11}}>{p}</span></div>
          <div style={{display:'flex',alignItems:'center',gap:8}}><span style={{flex:1}}><Bar v={cpu*2}/></span><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-body)',width:32,textAlign:'right'}}>{jitter(cpu)}%</span></div>
          <span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-muted)',textAlign:'right'}}>{mem}</span>
        </div>)}
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:12}}>
        <div style={{...card,padding:'16px 18px',display:'flex',flexDirection:'column',gap:18}}>
          <div style={{display:'flex',alignItems:'baseline',justifyContent:'space-between'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Répartition du stockage</span><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{diskUsed} Go</span></div>
          <SegmentedBar segments={[{value:38,name:'Images',color:'var(--viz-1)'},{value:31,name:'Volumes',color:'var(--viz-2)'},{value:19,name:'Sauvegardes',color:'var(--viz-3)'},{value:12,name:'Logs',color:'var(--viz-4)'}]}/>
        </div>
        <div style={{...card,overflow:'hidden'}}>
          <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'14px 18px'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Alertes</span><Badge tone="negative">{ALERTS.filter(a=>a[0]==='error').length} actives</Badge></div>
          {ALERTS.map(([st,t,tg,w],i)=><div key={i} style={{display:'flex',alignItems:'center',gap:10,padding:'10px 18px',borderTop:hair}}>
            <StatusDot status={st}/><span style={{flex:1,minWidth:0,font:'400 12px/1.2 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{t}</span>{mono(tg,'var(--text-subtle)')}<span style={{...lbl,fontSize:11,width:62,textAlign:'right'}}>{w}</span>
          </div>)}
        </div>
      </div>
    </div>
  </div>;
}
window.MonitoringPage=MonitoringPage;
})();
