(()=>{
const {Button,IconButton,Icon,Select,Switch}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const line='1px solid rgba(255,255,255,.06)';
const box={borderRadius:16,border:'1px solid rgba(255,255,255,.08)',background:'#0c0c0f'};
const mono=(v,c='var(--text-body)',s=12)=><span style={{font:'400 '+s+'px/1 var(--font-mono)',color:c,whiteSpace:'nowrap'}}>{v}</span>;
const ST={ok:['#86efac','circle-check'],error:['#f87171','circle-x'],pending:['#fbbf24','loader'],off:['#71717a','circle-minus']};
function SIcon({s,size=14}){const [c,ic]=ST[s]||ST.off;return <Icon name={ic} size={size} color={c} style={s==='pending'?{animation:'spin 1.2s linear infinite'}:undefined}/>}
function Spk({d,col='237,237,237',w=88,h=32}){const m=Math.max(...d),n=Math.min(...d),pts=d.map((v,i)=>[i/(d.length-1)*w,h-2-(v-n)/((m-n)||1)*(h-6)]);const p=pts.map((x,i)=>(i?'L':'M')+x[0].toFixed(1)+','+x[1].toFixed(1)).join(' ');return <svg width={w} height={h} style={{flex:'none'}}><path d={p+' L'+w+','+h+' L0,'+h+' Z'} fill={'rgba('+col+',.14)'}/><path d={p} fill="none" stroke={'rgb('+col+')'} strokeWidth="1.5"/></svg>}
function Tabs({tabs,value,onChange,right}){return <div style={{display:'flex',alignItems:'center',gap:22,borderBottom:'1px solid rgba(255,255,255,.08)'}}>{tabs.map(([k,n,c])=>{const on=value===k;return <button key={k} onClick={()=>onChange(k)} style={{height:38,padding:0,border:0,borderBottom:'2px solid '+(on?'#ededed':'transparent'),marginBottom:-1,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{n}{c!=null&&<span style={{fontWeight:400,color:'var(--text-subtle)'}}> {c}</span>}</button>})}<span style={{flex:1}}></span>{right}</div>}
function Search({v,set,ph,w=260}){return <div style={{display:'flex',alignItems:'center',gap:8,height:34,width:w,maxWidth:'100%',padding:'0 12px',marginBottom:4,borderRadius:10,border:'1px solid rgba(255,255,255,.1)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={v} onChange={e=>set(e.target.value)} placeholder={ph} style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>}
function Toast({t}){return t?<div style={{position:'fixed',bottom:24,right:24,zIndex:80,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{t}</div>:null}
function useToast(){const [t,s]=React.useState('');const r=React.useRef();return [t,m=>{s(m);clearTimeout(r.current);r.current=setTimeout(()=>s(''),2200)}]}
const AV=n=>{const i=n.split(' ').map(x=>x[0]).join('').slice(0,2);const h=[...n].reduce((a,c)=>a+c.charCodeAt(0)*7,0)%360;return <span title={n} style={{width:22,height:22,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',font:'600 9px/1 var(--font-sans)',color:'#0b0b0b',background:'oklch(0.82 0.07 '+h+')'}}>{i}</span>};

/* ---------- Vue d'ensemble ---------- */
function MiniBars({d,col,today}){const m=Math.max(...d);return <div style={{display:'flex',alignItems:'flex-end',gap:2,height:44}}>{d.map((v,i)=><span key={i} style={{width:3,height:Math.max(3,v/m*44),borderRadius:2,background:i>=today?'rgba(255,255,255,.12)':'rgb('+col+')',opacity:i>=today?1:.45+.55*(i/today)}}></span>)}</div>}
function DotMatrix({data,today,hover,setHover}){
  const rows=17,max=Math.max(...data.map(d=>d[0]+d[1]+d[2]));const W=26,H=12;const C=['134,239,172','251,146,60','250,204,21'];
  return <div style={{position:'relative'}} onMouseLeave={()=>setHover(null)}>
    <svg viewBox={'0 0 '+(data.length*W)+' '+(rows*H+4)} style={{width:'100%',height:'auto',display:'block'}}>
      {data.map((d,c)=>{const tot=Math.round((d[0]+d[1]+d[2])/max*rows);const g=Math.round(d[0]/max*rows),o=Math.round(d[1]/max*rows);const fut=c>=today;
        return <g key={c} onMouseEnter={()=>setHover(c)} style={{cursor:'crosshair'}}><rect x={c*W} y={0} width={W} height={rows*H+4} fill="transparent"/>{Array.from({length:rows},(_,r)=>{const fromBot=rows-1-r;const on=fromBot<tot;const col=fromBot<g?C[0]:fromBot<g+o?C[1]:C[2];return <circle key={r} cx={c*W+W/2} cy={r*H+H/2+2} r={3.4} fill={fut?(on?'rgba(255,255,255,.22)':'rgba(255,255,255,.06)'):on?'rgb('+col+')':'rgba(255,255,255,.05)'} opacity={hover!=null&&hover!==c&&!fut?.45:1}/>})}</g>})}
      <line x1={today*W} x2={today*W} y1={0} y2={rows*H+4} stroke="#ededed" strokeWidth="1.5"/>
    </svg>
  </div>;
}
function AreaChart({series,labels,fmt,col}){
  const ref=React.useRef();const [w,setW]=React.useState(800);const [i,setI]=React.useState(null);const H=240;
  React.useEffect(()=>{const ro=new ResizeObserver(e=>setW(e[0].contentRect.width));ro.observe(ref.current);return()=>ro.disconnect()},[]);
  const max=Math.max(...series)*1.15,min=0,n=series.length;const x=k=>k/(n-1)*w,y=v=>H-(v-min)/(max-min)*H;
  const pts=series.map((v,k)=>[x(k),y(v)]);
  const path=pts.map((q,k)=>{if(!k)return 'M'+q[0]+','+q[1];const pr=pts[k-1];const cx=(pr[0]+q[0])/2;return 'C'+cx+','+pr[1]+' '+cx+','+q[1]+' '+q[0]+','+q[1]}).join(' ');
  const ticks=[.25,.5,.75,1].map(f=>max*f);
  return <div ref={ref} style={{position:'relative',height:H+28}} onMouseMove={e=>{const r=ref.current.getBoundingClientRect();setI(Math.max(0,Math.min(n-1,Math.round((e.clientX-r.left)/w*(n-1)))))}} onMouseLeave={()=>setI(null)}>
    <svg width={w} height={H} style={{position:'absolute',left:0,top:0,overflow:'visible'}}>
      <defs><linearGradient id="ovA" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stopColor={col} stopOpacity=".16"/><stop offset="1" stopColor={col} stopOpacity="0"/></linearGradient></defs>
      {ticks.map(v=><g key={v}><line x1="0" x2={w} y1={y(v)} y2={y(v)} stroke="rgba(255,255,255,.05)"/><text x={w} y={y(v)-6} textAnchor="end" fill="rgba(255,255,255,.3)" style={{font:'400 10px var(--font-mono)'}}>{fmt(v)}</text></g>)}
      <path d={path+' L'+w+','+H+' L0,'+H+' Z'} fill="url(#ovA)"/>
      <path d={path} fill="none" stroke={col} strokeWidth="1.75"/>
      {i!=null&&<><line x1={x(i)} x2={x(i)} y1={y(series[i])+8} y2={H} stroke={col} strokeOpacity=".35" strokeDasharray="3 3"/><circle cx={x(i)} cy={y(series[i])} r="9" fill={col} fillOpacity=".15"/><circle cx={x(i)} cy={y(series[i])} r="4" fill={col} stroke="#0b0b0b" strokeWidth="2"/></>}
    </svg>
    {i!=null&&<div style={{position:'absolute',top:Math.max(-6,y(series[i])-46),left:Math.min(w-85,Math.max(85,x(i))),transform:'translateX(-50%)',display:'flex',alignItems:'center',gap:8,height:30,padding:'0 10px',borderRadius:99,background:'#1b1b20',border:'1px solid rgba(255,255,255,.1)',boxShadow:'0 8px 20px -8px rgba(0,0,0,.8)',pointerEvents:'none',whiteSpace:'nowrap'}}><span style={{width:7,height:7,borderRadius:99,background:col}}></span><span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{fmt(series[i])}</span><span style={{width:1,height:12,background:'rgba(255,255,255,.12)'}}></span><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-muted)'}}>{labels[i]}</span></div>}
    <div style={{position:'absolute',left:0,right:0,bottom:0,display:'flex',justifyContent:'space-between',font:'400 11px/1 var(--font-sans)',color:'var(--text-faint)'}}>{[0,.25,.5,.75,1].map(f=><span key={f}>{labels[Math.round(f*(n-1))]}</span>)}</div>
  </div>;
}
function OverviewV2({go}){
  const D=window.CIPHERA;const P=D.projects,S=D.servers,DE=D.deployments;const {Badge}=window.CipheraDesignSystem;
  const [m,setM]=React.useState('dep');const [h,setH]=React.useState(null);
  const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};const hair='1px solid var(--border-subtle)';
  const MET={dep:['Déploiements',D.deploys,v=>Math.round(v)+'','#a78bfa','1 486','+12,4 %',true],req:['Requêtes',D.deploys.map((v,i)=>v*1100+((i*37)%9)*900),v=>(v/1000).toFixed(0)+' k','#7dd3fc','1,62 M','+8,1 %',true],lat:['Latence p95',D.latency,v=>Math.round(v)+' ms','#86efac','182 ms','+4 ms',false]};
  const M=MET[m];
  const Tile=({ic})=><span style={{position:'relative',width:56,height:56,flex:'none'}}><span style={{position:'absolute',left:6,right:6,bottom:-8,height:10,borderRadius:'50%',background:'rgba(0,0,0,.55)',filter:'blur(5px)'}}></span><span style={{position:'absolute',inset:0,borderRadius:15,transform:'rotate(-10deg)',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18),inset 0 -2px 0 rgba(0,0,0,.35)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name={ic} size={23} color="#f2f2f2"/></span></span>;
  const seg=(v,n=4)=>{const on=Math.round(v/100*n);return <span style={{display:'flex',gap:4}}>{Array.from({length:n},(_,i)=><span key={i} style={{width:28,height:5,borderRadius:99,background:i<on?(v>=85?'#f87171':'#8b7cf6'):'var(--surface-selected)'}}></span>)}</span>};
  const ST={ok:['circle-check','var(--green-500)','Réussi'],pending:['circle-alert','#e5b454','En cours'],error:['circle-x','#f87171','Échec']};
  const link=(t,to)=><button onClick={()=>go&&go(to)} style={{display:'flex',alignItems:'center',gap:4,padding:0,border:0,background:'transparent',cursor:'pointer',font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{t}<Icon name="arrow-right" size={12}/></button>;
  return <div style={{padding:'20px 32px 48px 34px',display:'flex',flexDirection:'column',gap:14}}>
    {(()=>{const errP=P.filter(x=>x.status[0]==='error'),offS=S.filter(x=>x.status[0]==='error');const okAll=!errP.length&&!offS.length;
      const hr=new Date().getHours();const hello=hr<6?'Bonne nuit':hr<18?'Bonjour':'Bonsoir';
      const date=new Date().toLocaleDateString('fr-FR',{weekday:'long',day:'numeric',month:'long'});
      const ISS=[...errP.map(x=>['circle-x','#f87171',x.name,'Dernier build en échec','Projets']),...offS.map(x=>['unplug','#e5b454',x.name,'Agent injoignable','Serveurs'])];
      return <div style={{...card,borderRadius:20,position:'relative',overflow:'hidden'}}>
        <span style={{position:'absolute',inset:0,background:'radial-gradient(55% 120% at 0% 0%,rgba(255,255,255,.06),transparent 70%)',pointerEvents:'none'}}></span>
        <span style={{position:'absolute',top:0,right:0,bottom:0,width:'46%',backgroundImage:'radial-gradient(rgba(255,255,255,.07) 1px,transparent 1px)',backgroundSize:'16px 16px',maskImage:'linear-gradient(90deg,transparent,#000 60%)',WebkitMaskImage:'linear-gradient(90deg,transparent,#000 60%)',pointerEvents:'none'}}></span>
        <div style={{position:'relative',display:'grid',gridTemplateColumns:'minmax(0,1fr) minmax(0,360px)',gap:28,padding:'26px 28px'}}>
          <div style={{display:'flex',flexDirection:'column',gap:14,minWidth:0}}>
            <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)',textTransform:'capitalize'}}>{date}</span>
            <span style={{font:'600 26px/1.15 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.02em'}}>{hello} Alex</span>
            <span style={{display:'flex',alignItems:'center',gap:8,font:'400 14px/1.5 var(--font-sans)',color:'var(--text-muted)'}}><span style={{width:8,height:8,borderRadius:99,flex:'none',background:okAll?'var(--green-500)':'#e5b454'}}></span>{okAll?'Tous vos services fonctionnent normalement.':ISS.length+' point'+(ISS.length>1?'s':'')+' à traiter, le reste fonctionne normalement.'}</span>
            <div style={{display:'flex',alignItems:'center',gap:0,marginTop:8}}>
              {[['Projets en ligne',P.filter(x=>x.status[0]==='ok').length,'/ '+P.length],['Serveurs',S.filter(x=>x.status[0]!=='error').length,'/ '+S.length],['Déploiements aujourd\'hui','14','']].map(([k,v,u],i)=><div key={k} style={{display:'flex',flexDirection:'column',gap:9,padding:i?'0 24px':'0 24px 0 0',borderLeft:i?'1px solid var(--border-subtle)':'none'}}><span style={{...lbl,whiteSpace:'nowrap'}}>{k}</span><span style={{display:'flex',alignItems:'baseline',gap:5}}><span style={{font:'600 22px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{u}</span></span></div>)}
            </div>
            <div style={{display:'flex',gap:8,marginTop:10}}>
              <Button variant="inverse" icon="plus" onClick={()=>go&&go('Projets')}>Nouveau projet</Button>
              <Button icon="rocket" onClick={()=>go&&go('Déploiements')}>Déploiements</Button>
            </div>
          </div>
          <div style={{display:'flex',flexDirection:'column',borderRadius:14,border:'1px solid var(--border-default)',background:'rgba(11,11,13,.7)',overflow:'hidden',alignSelf:'start'}}>
            <div style={{display:'flex',alignItems:'center',padding:'12px 14px',borderBottom:'1px solid var(--border-subtle)'}}><span style={{flex:1,font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>À traiter</span><span style={{display:'inline-flex',alignItems:'center',height:20,padding:'0 7px',borderRadius:6,background:ISS.length?'rgba(229,180,84,.12)':'rgba(34,197,94,.12)',font:'500 11px/1 var(--font-mono)',color:ISS.length?'#e5b454':'var(--green-500)'}}>{ISS.length}</span></div>
            {ISS.length?ISS.map(([ic,c,n,d,to])=><button key={n} onClick={()=>go&&go(to)} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-raised)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',border:0,borderTop:'1px solid var(--border-subtle)',background:'transparent',cursor:'pointer',textAlign:'left'}}>
              <span style={{width:32,height:32,borderRadius:9,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={ic} size={15} color={c}/></span>
              <span style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}>{mono(n,'var(--text-strong)',13)}<span style={{...lbl,fontSize:11}}>{d}</span></span>
              <Icon name="chevron-right" size={14} color="var(--text-subtle)"/>
            </button>):<div style={{display:'flex',alignItems:'center',gap:10,padding:'16px 14px',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="circle-check" size={15} color="var(--green-500)"/>Rien à signaler</div>}
          </div>
        </div>
      </div>})()}
    <div style={{...card,borderRadius:18,padding:'20px 22px 18px',display:'flex',flexDirection:'column',gap:20}}>
      <div style={{display:'flex',alignItems:'stretch',gap:0}}>
        {Object.entries(MET).map(([k,v],idx)=>{const on=m===k;return <button key={k} onClick={()=>setM(k)} style={{display:'flex',flexDirection:'column',gap:10,padding:'4px 28px 4px '+(idx?'28px':'0'),border:0,borderLeft:idx?hair:'none',background:'transparent',cursor:'pointer',textAlign:'left',opacity:on?1:.5,transition:'opacity .15s'}}>
          <span style={{display:'flex',alignItems:'center',gap:7,...lbl}}><span style={{width:8,height:8,borderRadius:3,background:v[3]}}></span>{v[0]}</span>
          <span style={{display:'flex',alignItems:'baseline',gap:10}}><span style={{font:'600 26px/1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>{v[4]}</span><span style={{font:'500 12px/1 var(--font-sans)',color:v[6]?'#86efac':'#f87171'}}>{v[5]}</span></span>
        </button>})}
        <span style={{flex:1}}></span>
        <span style={{alignSelf:'flex-start',display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:'var(--text-body)'}}>30 derniers jours</span>
      </div>
      <AreaChart key={m} series={M[1]} labels={D.days} fmt={M[2]} col={M[3]}/>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.4fr) minmax(0,1fr)',gap:14}}>
      <div style={{...card,borderRadius:18,overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',padding:'16px 20px'}}><span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Déploiements récents</span><span style={{flex:1}}></span>{link('Tout voir','Déploiements')}</div>
        {DE.slice(0,5).map(d=>{const st=ST[d.s[0]]||['circle-minus','var(--text-subtle)','Annulé'];return <div key={d.id} onMouseEnter={()=>setH(d.id)} onMouseLeave={()=>setH(null)} style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) 110px 90px',gap:16,alignItems:'center',padding:'14px 20px',borderTop:hair,background:h===d.id?'var(--surface-raised)':'transparent'}}>
          <div style={{display:'flex',alignItems:'center',gap:14,minWidth:0}}>
            <span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name="git-commit-horizontal" size={18} color="var(--text-strong)"/></span>
            <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}><span style={{font:'500 14px/1.1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{d.msg}</span><span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{d.author} · {d.id} · {d.commit}</span></div>
          </div>
          <span style={{display:'flex',alignItems:'center',gap:7}}><Icon name={st[0]} size={15} color={st[1]}/><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{st[2]}</span></span>
          <span style={{...lbl,textAlign:'right',whiteSpace:'nowrap'}}>{d.when}</span>
        </div>})}
      </div>
      <div style={{...card,borderRadius:18,overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',padding:'16px 20px'}}><span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Charge des serveurs</span><span style={{flex:1}}></span>{link('Serveurs','Serveurs')}</div>
        {S.map(s=>{const off=s.status[0]==='error';const c=off?0:s.cpu;return <div key={s.id} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 20px',borderTop:hair}}>
          {window.OsLogo?<window.OsLogo os={s.os} size={40} dim={off}/>:null}
          <div style={{display:'flex',flexDirection:'column',gap:9,flex:1,minWidth:0}}>
            <span style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}>{mono(s.name,'var(--text-strong)',13)}{off&&<Badge>Hors ligne</Badge>}</span>
            <span style={{display:'flex',alignItems:'center',gap:10}}>{seg(c)}<span style={{...lbl,fontSize:11}}>{off?'—':c+' % CPU'}</span></span>
          </div>
        </div>})}
      </div>
    </div>
  </div>;
}

const DEPS=[
  {id:4822,msg:'feat: graphiques de latence',sha:'a3f91c2',p:'ciphera-web',br:'main',env:'Production',s:'pending',by:'Alex Martin',dur:null,when:'il y a 2 min',prog:62},
  {id:4821,msg:'feat(charts): tooltip synchronisé',sha:'e19b44d',p:'dashboard-v2',br:'feat/charts',env:'Preview',s:'pending',by:'Yanis Benali',dur:null,when:'il y a 3 min',prog:24},
  {id:4818,msg:'fix: cache des en-têtes',sha:'7be0d14',p:'ciphera-web',br:'main',env:'Production',s:'ok',by:'Lina Moreau',dur:'38 s',when:'il y a 4 h'},
  {id:4815,msg:'chore: dépendances de sécurité',sha:'5f2a0c1',p:'auth-service',br:'main',env:'Production',s:'ok',by:'Camille Roux',dur:'1 min 04 s',when:'il y a 6 h'},
  {id:4812,msg:'chore: mise à jour Node 22',sha:'c41a9e0',p:'billing-api',br:'main',env:'Production',s:'ok',by:'Yanis Benali',dur:'52 s',when:'hier'},
  {id:4809,msg:'feat: nouvelle page tarifs',sha:'b8e3d21',p:'marketing-site',br:'staging',env:'Staging',s:'ok',by:'Hugo Lefèvre',dur:'41 s',when:'hier'},
  {id:4807,msg:'feat: page tarifs',sha:'09d2bb7',p:'billing-api',br:'main',env:'Production',s:'error',by:'Alex Martin',dur:'41 s',when:'il y a 2 j',err:'pnpm install a échoué (ERR_PNPM_LOCKFILE)'},
  {id:4803,msg:'revert: bascule CDN',sha:'1d0f7aa',p:'ciphera-web',br:'main',env:'Production',s:'off',by:'Lina Moreau',dur:'18 s',when:'il y a 3 j'},
];
const ENVC={Production:'134,239,172',Preview:'139,124,246',Staging:'251,191,36'};
function DMenu({items,onClose}){
  React.useEffect(()=>{const f=()=>onClose();setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[]);
  return <div style={{position:'absolute',top:34,right:0,zIndex:30,width:220,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>{items.map((it,i)=>it==='-'?<div key={i} style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>:<button key={i} onClick={it[2]} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',textAlign:'left',color:it[3]?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={it[0]} size={14}/>{it[1]}</button>)}</div>;
}
const DCOLS='28px minmax(0,1.7fr) minmax(0,1fr) minmax(0,1fr) 120px 150px';
const DST={ok:['circle-check','var(--green-500)','Réussi'],pending:['circle-alert','#e5b454','En cours'],error:['circle-x','#f87171','Échec'],off:['circle-minus','var(--text-subtle)','Annulé']};
function DeployRow({d,open,onToggle,flash,redeploy,cancel}){
  const [h,setH]=React.useState(false);const [menu,setMenu]=React.useState(false);const st=DST[d.s];
  const {Button,IconButton,Badge}=window.CipheraDesignSystem;
  const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)'};
  const STG=[['Clonage','2 s'],['Build',d.s==='error'?'échec':'38 s'],['Image','9 s'],['Bascule','3 s']];
  return <div style={{borderTop:'1px solid var(--border-subtle)'}} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)}>
    <div onClick={onToggle} style={{display:'grid',gridTemplateColumns:DCOLS,gap:16,alignItems:'center',padding:'14px 16px',cursor:'pointer',background:h&&!open?'var(--surface-raised)':'transparent'}}>
      <Icon name="chevron-down" size={15} color="var(--text-muted)" style={{transform:open?'rotate(180deg)':'none',transition:'transform var(--dur-fast)'}}/>
      <div style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}>
        <span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',border:'1px solid var(--border-default)'}}><Icon name="git-commit-horizontal" size={18} color="var(--text-strong)"/></span>
        <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}>
          <span style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}><span style={{font:'500 13px/1.2 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{d.msg}</span>{d.env!=='Production'&&<span style={{flex:'none'}}><Badge>{d.env}</Badge></span>}</span>
          <span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>#{d.id} · {d.sha} · {d.br}</span>
        </div>
      </div>
      <span style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}>{mono(d.p,'var(--text-body)')}</span>
      <span style={{display:'flex',alignItems:'center',gap:8}}><Icon name={st[0]} size={16} color={st[1]}/><span style={{font:'400 13px/1.2 var(--font-sans)',color:'var(--text-body)'}}>{d.s==='pending'?'En cours · '+d.prog+' %':st[2]}</span></span>
      <span><span style={{display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:d.s==='error'?'rgba(229,180,84,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:d.s==='error'?'#e5b454':'var(--text-body)'}}>{d.when}</span></span>
      <div style={{display:'flex',gap:6,justifyContent:'flex-end',position:'relative'}} onClick={e=>e.stopPropagation()}>
        {d.s==='pending'?<Button size="sm" onClick={cancel}>Annuler</Button>:<Button size="sm" onClick={()=>flash('Logs de #'+d.id)}>Logs</Button>}
        <IconButton size={28} icon="ellipsis-vertical" label="Actions" onClick={()=>setMenu(m=>!m)}/>
        {menu&&<DMenu onClose={()=>setMenu(false)} items={[['rotate-cw','Redéployer',redeploy],['undo-2','Revenir à cette version',()=>flash('Rollback vers #'+d.id)],['copy','Copier le SHA',()=>{navigator.clipboard&&navigator.clipboard.writeText(d.sha);flash('SHA copié')}],['git-branch','Voir le commit',()=>flash('github.com/ciphera/…/commit/'+d.sha)]]}/>}
      </div>
    </div>
    {open&&<div style={{padding:'0 16px 16px'}}>
      {d.err?<div style={{display:'flex',alignItems:'center',gap:12,padding:'14px 18px',borderRadius:12,background:'rgba(239,68,68,.05)',border:'1px solid rgba(239,68,68,.18)',marginBottom:10}}><Icon name="circle-x" size={15} color="#f87171"/><span style={{flex:1,font:'400 12px/1.4 var(--font-sans)',color:'var(--text-body)'}}>{d.err}</span><Button size="sm" onClick={redeploy}>Relancer</Button></div>:null}
      <div style={{display:'grid',gridTemplateColumns:'repeat(5,minmax(0,1fr))',gap:24,padding:'16px 22px 18px',borderRadius:12,background:'var(--surface-raised)'}}>
        {[['Auteur',d.by],['Durée',d.dur||'—'],...STG.slice(0,3).map(([k,v])=>[k,d.s==='pending'?'…':v])].map(([k,v])=><div key={k} style={{display:'flex',flexDirection:'column',gap:10,minWidth:0}}><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{k}</span><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{v}</span></div>)}
      </div>
    </div>}
  </div>;
}
function DeployPicker({list,onPick}){
  const {Button}=window.CipheraDesignSystem;const [open,setOpen]=React.useState(false);const ref=React.useRef();const pop=React.useRef();const [pos,setPos]=React.useState({top:0,right:0});
  React.useEffect(()=>{if(!open)return;const r=ref.current.getBoundingClientRect();setPos({top:r.bottom+8,right:window.innerWidth-r.right});const f=e=>{if(ref.current&&!ref.current.contains(e.target)&&!(pop.current&&pop.current.contains(e.target)))setOpen(false)};document.addEventListener('mousedown',f);return()=>document.removeEventListener('mousedown',f)},[open]);
  const P=window.CIPHERA.projects;
  return <div ref={ref} style={{position:'relative'}}>
    <Button variant="inverse" icon="rocket" onClick={()=>setOpen(o=>!o)}>Déployer un projet</Button>
    {open&&ReactDOM.createPortal(<div ref={pop} style={{position:'fixed',top:pos.top,right:pos.right,zIndex:1000,width:300,padding:6,borderRadius:'var(--radius-lg)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)'}}>
      <span style={{display:'block',padding:'8px 10px 6px',font:'600 10px/1 var(--font-sans)',letterSpacing:'.08em',textTransform:'uppercase',color:'var(--text-subtle)'}}>Choisir un projet</span>
      {P.map(x=>{const last=list.find(d=>d.p===x.name);return <button key={x.id} onClick={()=>{setOpen(false);onPick(last||{id:0,msg:'Déploiement manuel',sha:'—',p:x.name,br:x.branch,env:x.env,s:'ok',by:'Alex Martin'})}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'9px 10px',border:0,borderRadius:8,background:'transparent',cursor:'pointer',textAlign:'left'}}>
        <span style={{width:30,height:30,borderRadius:8,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name="layers" size={14} color="var(--text-strong)"/></span>
        <span style={{display:'flex',flexDirection:'column',gap:5,flex:1,minWidth:0}}>{mono(x.name,'var(--text-strong)',13)}<span style={{...lbl,fontSize:11}}>{x.branch} · {x.env}</span></span>
        <Icon name="rotate-cw" size={13} color="var(--text-subtle)"/>
      </button>})}
    </div>,document.body)}
  </div>;
}
function DeploymentsPage(){
  const {Tabs:DTabs}=window.CipheraDesignSystem;
  const [list,setList]=React.useState(DEPS);const [tab,setTab]=React.useState('all');const [exp,setExp]=React.useState(null);const [sort,setSort]=React.useState(['id',-1]);const [t,flash]=useToast();
  React.useEffect(()=>{const id=setInterval(()=>setList(l=>l.map(d=>d.s==='pending'?(d.prog>=100?{...d,s:'ok',dur:'1 min 12 s',when:"à l'instant",prog:100}:{...d,prog:Math.min(100,d.prog+7)}):d)),900);return()=>clearInterval(id)},[]);
  const F={all:()=>true,run:d=>d.s==='pending',ok:d=>d.s==='ok',err:d=>d.s==='error'};
  const rows=[...list.filter(F[tab])].sort((x,y)=>{const k=sort[0];return (x[k]>y[k]?1:-1)*sort[1]});
  const redeploy=d=>{const id=Math.max(...list.map(x=>x.id))+1;setList(l=>[{...d,id,s:'pending',prog:4,dur:null,when:"à l'instant",by:'Alex Martin',err:null},...l]);flash('#'+id+' lancé · '+d.p)};
  const H=(k,t2)=><button onClick={()=>setSort(s=>[k,s[0]===k?-s[1]:1])} style={{display:'flex',alignItems:'center',gap:6,padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 12px/1 var(--font-sans)',color:sort[0]===k?'var(--text-strong)':'var(--text-muted)'}}>{t2}<Icon name="arrow-down-up" size={12}/></button>;
  const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
  const D=window.CIPHERA;const [m,setM]=React.useState('dep');const hair='1px solid var(--border-subtle)';
  const MET={dep:['Déploiements / jour',D.deploys,v=>Math.round(v)+'','#a78bfa','71'],ok:['Taux de réussite',D.deploys.map((v,i)=>94+((i*7)%5)),v=>v.toFixed(0)+' %','#86efac','97,3 %'],dur:['Durée moyenne',D.deploys.map((v,i)=>52-i*.3+((i*11)%7)),v=>Math.round(v)+' s','#7dd3fc','42 s']};
  const M=MET[m];const run=list.filter(F.run).length,err=list.filter(F.err).length;
  const Tile=({ic})=><span style={{position:'relative',width:56,height:56,flex:'none'}}><span style={{position:'absolute',left:6,right:6,bottom:-8,height:10,borderRadius:'50%',background:'rgba(0,0,0,.55)',filter:'blur(5px)'}}></span><span style={{position:'absolute',inset:0,borderRadius:15,transform:'rotate(-10deg)',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18),inset 0 -2px 0 rgba(0,0,0,.35)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name={ic} size={23} color="#f2f2f2"/></span></span>;
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:14}}>
    {(()=>{const cur=list.find(d=>d.s==='pending');return <div style={{...card,borderRadius:18,position:'relative',overflow:'hidden',display:'flex',alignItems:'center',gap:24,padding:'26px 28px'}}>
      <span style={{position:'absolute',inset:0,background:'radial-gradient(50% 140% at 100% 0%,rgba(56,189,248,.14),transparent 70%)',pointerEvents:'none'}}></span>
      <span style={{position:'relative',display:'flex',flexDirection:'column',gap:10,flex:1,minWidth:0}}><span style={{font:'600 18px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Chaque push en ligne en moins d'une minute</span><span style={{font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)',maxWidth:520}}>Build, image et bascule sans coupure. {run?run+' déploiement'+(run>1?'s':'')+' en cours':'Aucun déploiement en cours'} · {err} échec{err>1?'s':''} cette semaine.</span></span>
      <DeployPicker list={list} onPick={redeploy}/>
    </div>})()}
    <div style={{...card,borderRadius:18,padding:'20px 22px 18px',display:'flex',flexDirection:'column',gap:20}}>
      <div style={{display:'flex',alignItems:'stretch'}}>{Object.entries(MET).map(([k,v],idx)=>{const on=m===k;return <button key={k} onClick={()=>setM(k)} style={{display:'flex',flexDirection:'column',gap:10,padding:'4px 28px 4px '+(idx?'28px':'0'),border:0,borderLeft:idx?hair:'none',background:'transparent',cursor:'pointer',textAlign:'left',opacity:on?1:.5,transition:'opacity .15s'}}><span style={{display:'flex',alignItems:'center',gap:7,...lbl}}><span style={{width:8,height:8,borderRadius:3,background:v[3]}}></span>{v[0]}</span><span style={{font:'600 26px/1 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>{v[4]}</span></button>})}<span style={{flex:1}}></span><span style={{alignSelf:'flex-start',display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:'var(--text-body)'}}>30 derniers jours</span></div>
      <AreaChart key={m} series={M[1]} labels={D.days} fmt={M[2]} col={M[3]}/>
    </div>
    <div style={{paddingLeft:12,marginTop:6}}><DTabs tabs={[{value:'all',label:'Tous',count:list.length},{value:'run',label:'En cours',count:list.filter(F.run).length},{value:'ok',label:'Réussis',count:list.filter(F.ok).length},{value:'err',label:'Échecs',count:list.filter(F.err).length}]} value={tab} onChange={setTab}/></div>
    <div style={{...card,overflow:'visible'}}>
      <div style={{display:'grid',gridTemplateColumns:DCOLS,gap:16,padding:'14px 16px'}}><span></span>{H('msg','Détails')}{H('p','Projet')}{H('s','Statut')}{H('id','Date')}<span></span></div>
      {rows.map(d=><DeployRow key={d.id} d={d} open={exp===d.id} onToggle={()=>setExp(x=>x===d.id?null:d.id)} flash={flash} redeploy={()=>redeploy(d)} cancel={()=>{setList(l=>l.map(x=>x.id===d.id?{...x,s:'off',dur:'—'}:x));flash('#'+d.id+' annulé')}}/>)}
      {!rows.length&&<div style={{padding:'28px 16px',borderTop:'1px solid var(--border-subtle)',...lbl}}>Aucun déploiement.</div>}
    </div>
    <Toast t={t}/>
  </div>;
}

/* ---------- Paramètres ---------- */
function Row({t,d,children,last}){return <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) minmax(0,1.2fr)',gap:24,alignItems:'center',padding:'18px 20px',borderTop:last==='first'?'none':line}}><span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 13px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span>{d&&<span style={{font:'400 12px/1.45 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span>}</span><div style={{display:'flex',justifyContent:'flex-end',gap:8,minWidth:0}}>{children}</div></div>}
const inp=(v,set,ph,mono)=><input value={v} onChange={e=>set(e.target.value)} placeholder={ph} style={{width:'100%',maxWidth:340,height:38,padding:'0 12px',borderRadius:10,border:'1px solid rgba(255,255,255,.1)',background:'rgba(255,255,255,.045)',color:'var(--text-strong)',font:'400 13px/1 '+(mono?'var(--font-mono)':'var(--font-sans)'),outline:'none'}}/>;
function SettingsV2({openSecret}){
  const [delOrg,setDelOrg]=React.useState(null);
  const [sec,setSec]=React.useState('general');const [t,flash]=useToast();
  const [s,set]=React.useState({name:'Ciphera',slug:'ciphera',region:'Paris · eu-west-3',tz:'Europe/Paris',lang:'Français',auto:true,preview:true,retention:'30 jours',mail:true,weekly:true,slack:false,sso:false,mfa:true,ipAllow:false,domain:'ciphera.mondomaine.com',https:true,nf:'404',nfUrl:'https://ciphera.mondomaine.com'});
  const u=(k,v)=>{set(x=>({...x,[k]:v}));if(k==='domain'){setDns('idle');setCfRec(false)}};
  const [access,setAccess]=React.useState('public');const [ts,setTs]=React.useState({on:true,key:'',tailnet:'',host:'ciphera',state:'off',publicOff:true});const [tsBusy,setTsBusy]=React.useState(false);
  const tsConnect=()=>{setTsBusy(true);setTimeout(()=>{setTsBusy(false);setTs(x=>({...x,state:'ok'}));flash('Panel connecté au tailnet '+ts.tailnet)},1400)};
  const [dns,setDns]=React.useState('ok');const [cfRec,setCfRec]=React.useState(false);const [cfBusy,setCfBusy]=React.useState(false);
  const cfOn=(()=>{try{return !!JSON.parse(localStorage.getItem('ciphera.cloudflare'))}catch(e){return false}})();
  const checkDns=()=>{setDns('checking');setTimeout(()=>setDns(/^[a-z0-9-]+(\.[a-z0-9-]+)+$/i.test(s.domain)&&!/test|exemple/.test(s.domain)?'ok':'fail'),1200)};
  const addCf=()=>{setCfBusy(true);setTimeout(()=>{setCfBusy(false);setCfRec(true);setDns('ok');flash('Enregistrement A créé dans Cloudflare')},1100)};
  const NAV=[['general','Général','settings'],['domain','Domaine du panel','globe'],['deploy','Déploiements','rocket'],['notif','Notifications','bell'],['security','Sécurité','lock'],['danger','Zone de danger','triangle-alert']];
  const CARD={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
  const card=(title,desc,children)=><div style={{...CARD,borderRadius:18,overflow:'hidden'}}><div style={{padding:'18px 22px',borderBottom:'1px solid var(--border-subtle)',display:'flex',flexDirection:'column',gap:7}}><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span>{desc&&<span style={lbl}>{desc}</span>}</div>{children}</div>;
  const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:20}}>
    <div style={{...CARD,borderRadius:20,padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',alignItems:'center',gap:16}}>
        <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#fff'}}><img src="../../web/src/assets/brand/ciphera-icon.svg" alt="" style={{width:28,filter:'invert(1)'}}/></span>
        <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>{s.name}</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:'rgba(139,124,246,.15)',font:'600 11px/1 var(--font-sans)',color:'#c4b5fd'}}>Pro</span></span><span style={lbl}>{s.slug}.ciphera.app · {s.region}</span></span>
        <div style={{display:'flex',borderRadius:10,overflow:'hidden'}}><button onClick={()=>flash('Paramètres enregistrés')} style={{height:38,padding:'0 16px',border:0,cursor:'pointer',background:'#8b7cf6',color:'#fff',font:'600 13px/1 var(--font-sans)'}}>Enregistrer</button></div>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
        <span>ID : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>org_7f2a4c</span></span>{sep}
        <span>Membres : <span style={{fontWeight:600,color:'var(--text-strong)'}}>6 / 10</span></span>{sep}
        <span>Fuseau : <span style={{color:'var(--text-strong)'}}>{s.tz}</span></span>{sep}
        <span>2FA : <span style={{fontWeight:600,color:s.mfa?'var(--green-500)':'#e5b454'}}>{s.mfa?'obligatoire':'facultative'}</span></span>
        <span style={{flex:1}}></span>
        
      </div>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'220px minmax(0,1fr)',gap:24,alignItems:'start'}}>
    <div style={{position:'sticky',top:12,display:'flex',flexDirection:'column',gap:2}}>{NAV.map(([k,n,ic])=>{const on=sec===k;return <button key={k} onClick={()=>setSec(k)} style={{display:'flex',alignItems:'center',gap:10,height:36,padding:'0 12px',borderRadius:9,border:0,cursor:'pointer',textAlign:'left',background:on?'rgba(255,255,255,.07)':'transparent',font:'500 13px/1 var(--font-sans)',color:k==='danger'?'#f87171':on?'var(--text-strong)':'var(--text-muted)'}}><Icon name={ic} size={14}/>{n}</button>})}</div>
    <div style={{display:'flex',flexDirection:'column',gap:16,maxWidth:860}}>
      {sec==='general'&&<>{card('Organisation','Informations visibles par les membres de l\'espace.',<>
        <Row t="Nom de l'organisation" last="first">{inp(s.name,v=>u('name',v),'Ciphera')}</Row>
        <Row t="Identifiant" d="Utilisé dans les URL et l'API.">{inp(s.slug,v=>u('slug',v.toLowerCase().replace(/[^a-z0-9-]/g,'')),'ciphera',true)}</Row>
        <Row t="Région par défaut" d="Région proposée pour les nouveaux projets."><Select options={['Paris · eu-west-3','Francfort · eu-central-1','Virginie · us-east-1']} value={s.region} onChange={v=>u('region',v)}/></Row>
        <Row t="Fuseau horaire"><Select options={['Europe/Paris','Europe/Zurich','UTC','America/New_York']} value={s.tz} onChange={v=>u('tz',v)}/></Row>
        <Row t="Langue"><Select options={['Français','English']} value={s.lang} onChange={v=>u('lang',v)}/></Row></>)}
        <div style={{display:'flex',justifyContent:'flex-end'}}><Button variant="inverse" onClick={()=>flash('Paramètres enregistrés')}>Enregistrer</Button></div></>}
      {sec==='domain'&&<>
        <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:10}}>{[['public','globe','Domaine public','Accessible sur Internet via votre domaine et HTTPS.'],['vpn','shield','Accès privé (VPN)','Uniquement depuis les appareils de votre tailnet Tailscale.']].map(([k,ic,t2,d])=>{const on=access===k;return <button key={k} onClick={()=>{if(k===access)return;setAccess(k);flash(k==='vpn'?'Mode VPN : le panel ne sera plus exposé publiquement':'Mode public : le panel est exposé via '+s.domain)}} style={{display:'flex',alignItems:'center',gap:14,padding:'16px 18px',borderRadius:16,cursor:'pointer',textAlign:'left',border:'1px solid '+(on?'var(--border-strong)':'var(--border-default)'),background:on?'var(--surface-raised)':'var(--surface-app)'}}><span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={ic} size={18} color="var(--text-strong)"/></span><span style={{display:'flex',flexDirection:'column',gap:7,flex:1}}><span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t2}</span><span style={{font:'400 12px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></span><span style={{width:18,height:18,borderRadius:99,flex:'none',border:'2px solid '+(on?'#8b7cf6':'var(--border-strong)'),display:'flex',alignItems:'center',justifyContent:'center'}}>{on&&<span style={{width:8,height:8,borderRadius:99,background:'#8b7cf6'}}></span>}</span></button>})}</div>
        {access==='public'&&card('Domaine principal','Adresse à laquelle l\'équipe accède au dashboard.',<>
        <Row t="Domaine du panel" d="Pointez un enregistrement A vers l'IP de ce serveur." last="first">{inp(s.domain,v=>u('domain',v.toLowerCase().replace(/^https?:\/\//,'').replace(/\/.*$/,'')),'panel.exemple.com',true)}</Row>
        <Row t="Vérification DNS" d={dns==='ok'?'Le domaine pointe bien vers ce serveur.':dns==='fail'?'Le domaine ne pointe pas encore vers 51.15.204.12.':dns==='checking'?'Résolution en cours…':'Pas encore vérifié.'}><span style={{display:'flex',alignItems:'center',gap:8,flexWrap:'wrap',justifyContent:'flex-end'}}>
          <span style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:6,background:dns==='ok'?'rgba(34,197,94,.12)':dns==='fail'?'rgba(248,113,113,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:dns==='ok'?'var(--green-500)':dns==='fail'?'#f87171':'var(--text-muted)'}}><Icon name={dns==='ok'?'circle-check':dns==='fail'?'circle-x':dns==='checking'?'loader':'circle-minus'} size={12} style={dns==='checking'?{animation:'spin 1s linear infinite'}:undefined}/>{dns==='ok'?'A → 51.15.204.12':dns==='fail'?'A → introuvable':dns==='checking'?'vérification…':'non vérifié'}</span>
          <Button size="sm" icon="refresh-cw" onClick={checkDns} disabled={dns==='checking'}>Vérifier le DNS</Button>
        </span></Row>
        <Row t="Cloudflare" d={cfOn?(cfRec?'Enregistrement A créé et proxifié dans votre zone.':'Créez l\'enregistrement automatiquement dans votre zone Cloudflare.'):'Connectez Cloudflare pour créer l\'enregistrement automatiquement.'}>{cfOn?(cfRec?<span style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:6,background:'rgba(243,128,32,.12)',font:'500 12px/1 var(--font-sans)',color:'#f6821f'}}><Icon name="cloud" size={12}/>Ajouté à Cloudflare</span>:<button onClick={addCf} disabled={cfBusy} style={{display:'flex',alignItems:'center',gap:8,height:32,padding:'0 12px',borderRadius:8,border:0,cursor:'pointer',background:'#f6821f',color:'#fff',font:'600 12px/1 var(--font-sans)'}}><Icon name={cfBusy?'loader':'cloud'} size={13} color="#fff" style={cfBusy?{animation:'spin 1s linear infinite'}:undefined}/>{cfBusy?'Création…':'Ajouter à Cloudflare'}</button>):<span style={{...lbl}}>Non connecté</span>}</Row>
        <Row t="HTTPS"><span style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:6,background:s.https&&dns==='ok'?'rgba(34,197,94,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-sans)',color:s.https&&dns==='ok'?'var(--green-500)':'var(--text-muted)'}}><Icon name={s.https&&dns==='ok'?'lock':'lock-open'} size={12}/>{s.https?(dns==='ok'?'Certificat valide':'En attente du DNS'):'Désactivé'}</span></Row>
        <Row t="Certificat HTTPS automatique" d="Émis et renouvelé via Let's Encrypt."><Switch checked={s.https} onChange={v=>u('https',v)}/></Row></>)}
        {access==='vpn'&&card('Accès privé via Tailscale','Protégez le panel : il n\'est joignable que depuis les appareils de votre tailnet.',<>
          {<>
            <Row last="first" t="Clé d'authentification" d="Admin Tailscale › Settings › Keys › Generate auth key (réutilisable, pré-approuvée).">{inp(ts.key,v=>setTs(x=>({...x,key:v.trim(),state:'off'})),'tskey-auth-…',true)}</Row>
            <Row t="Nom du tailnet" d="Visible dans Admin Tailscale › DNS, par exemple tail1234.ts.net.">{inp(ts.tailnet,v=>setTs(x=>({...x,tailnet:v.trim().toLowerCase(),state:'off'})),'tail1234.ts.net',true)}</Row>
            <Row t="Nom de la machine" d="Nom d'hôte du panel dans le tailnet (MagicDNS).">{inp(ts.host,v=>setTs(x=>({...x,host:v.toLowerCase().replace(/[^a-z0-9-]/g,''),state:'off'})),'ciphera',true)}</Row>
            <Row t="Connexion" d={ts.state==='ok'?'Le panel est joignable depuis vos appareils Tailscale.':'Vérifie la clé puis enregistre la machine dans le tailnet.'}><span style={{display:'flex',alignItems:'center',gap:8,flexWrap:'wrap',justifyContent:'flex-end'}}>
              {ts.state==='ok'?<span style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:6,background:'rgba(34,197,94,.12)',font:'500 12px/1 var(--font-mono)',color:'var(--green-500)'}}><Icon name="circle-check" size={12}/>100.84.12.7</span>:null}
              <Button size="sm" icon={tsBusy?'loader':ts.state==='ok'?'refresh-cw':'plug-zap'} disabled={tsBusy||!/^tskey-/.test(ts.key)||!/\.ts\.net$/.test(ts.tailnet)||!ts.host} onClick={tsConnect}>{tsBusy?'Connexion…':ts.state==='ok'?'Reconnecter':'Connecter'}</Button>
            </span></Row>
            {ts.state==='ok'&&<Row t="Adresse Tailscale"><span style={{display:'flex',alignItems:'center',gap:8}}><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>https://{ts.host}.{ts.tailnet}</span><IconButton size={28} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText('https://'+ts.host+'.'+ts.tailnet);flash('Adresse copiée')}}/></span></Row>}
            <div style={{display:'flex',alignItems:'center',gap:10,margin:'0 20px 16px',padding:'10px 12px',borderRadius:10,background:'var(--surface-raised)',font:'400 12px/1.4 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="info" size={14}/>En mode VPN, le domaine public et l'IP du serveur ne répondent plus. Gardez Tailscale actif sur vos appareils.</div>
          </>}
        </>)}
        {card('Liens invalides','Ce qui se passe quand quelqu\'un ouvre une adresse qui n\'existe pas sur le panel.',<div style={{display:'flex',flexDirection:'column',gap:8,padding:'16px 20px'}}>
          {[['404','Afficher la page 404','Page Ciphera avec recherche et retour au dashboard.','file-x'],['redirect','Rediriger vers une adresse','Envoie le visiteur vers l\'URL de votre choix (code 302).','corner-up-right'],['none','Ne rien faire','Réponse 404 vide, sans page ni redirection.','ban']].map(([k,t2,d,ic])=>{const on=s.nf===k;return <button key={k} onClick={()=>u('nf',k)} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 16px',borderRadius:12,cursor:'pointer',textAlign:'left',border:'1px solid '+(on?'var(--border-strong)':'var(--border-subtle)'),background:on?'var(--surface-raised)':'transparent'}}><span style={{width:36,height:36,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={ic} size={16} color="var(--text-strong)"/></span><span style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t2}</span><span style={{font:'400 12px/1.3 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></span><span style={{width:18,height:18,borderRadius:99,border:'2px solid '+(on?'#8b7cf6':'var(--border-strong)'),display:'flex',alignItems:'center',justifyContent:'center'}}>{on&&<span style={{width:8,height:8,borderRadius:99,background:'#8b7cf6'}}></span>}</span></button>})}
          {s.nf==='redirect'&&<div style={{display:'flex',alignItems:'center',gap:10,paddingTop:6}}><span style={{...lbl,whiteSpace:'nowrap'}}>Rediriger vers</span>{inp(s.nfUrl,v=>u('nfUrl',v),'https://exemple.com',true)}</div>}
          <div style={{display:'flex',alignItems:'center',gap:10,paddingTop:8}}><Button size="sm" variant="ghost" iconRight="arrow-up-right" onClick={()=>window.open(s.nf==='redirect'?s.nfUrl:'404.html','_blank')}>Tester avec une adresse invalide</Button></div>
        </div>)}
        <div style={{display:'flex',justifyContent:'flex-end'}}><Button variant="inverse" onClick={()=>flash('Domaine et redirections enregistrés')}>Enregistrer</Button></div></>}
      {sec==='deploy'&&card('Déploiements','Comportement par défaut pour tous les projets.',<>
        <Row t="Déploiement automatique" d="Chaque push sur la branche de production lance un build." last="first"><Switch checked={s.auto} onChange={v=>u('auto',v)}/></Row>
        <Row t="Previews de pull request" d="Un environnement éphémère par PR."><Switch checked={s.preview} onChange={v=>u('preview',v)}/></Row>
        <Row t="Rétention des builds" d="Durée de conservation des images et des logs."><Select options={['7 jours','30 jours','90 jours']} value={s.retention} onChange={v=>u('retention',v)}/></Row>
        <Row t="Secrets de build" d="Variables chiffrées disponibles pendant le build."><Button size="sm" icon="key-round" onClick={openSecret}>Gérer les secrets</Button></Row></>)}
      {sec==='notif'&&card('Notifications','Où l\'équipe est prévenue des événements importants.',<>
        <Row t="E-mail" d="Échecs de build, incidents, certificats." last="first"><Switch checked={s.mail} onChange={v=>u('mail',v)}/></Row>
        <Row t="Rapport hebdomadaire" d="Résumé de l'activité chaque lundi."><Switch checked={s.weekly} onChange={v=>u('weekly',v)}/></Row>
        <Row t="Webhook Slack" d="Nécessite un connecteur Slack."><Switch checked={s.slack} onChange={v=>u('slack',v)}/></Row></>)}
      {sec==='security'&&card('Sécurité de l\'organisation','S\'applique à tous les membres.',<>
        <Row t="Double authentification obligatoire" last="first"><Switch checked={s.mfa} onChange={v=>u('mfa',v)}/></Row>
        <Row t="Authentification unique (SSO)" d="SAML ou OIDC."><Switch checked={s.sso} onChange={v=>u('sso',v)}/></Row>
        <Row t="Restreindre l'accès par IP" d="Seules les IP autorisées accèdent au tableau de bord."><Switch checked={s.ipAllow} onChange={v=>u('ipAllow',v)}/></Row></>)}
      {sec==='danger'&&<div style={{borderRadius:16,border:'1px solid rgba(248,113,113,.3)',background:'rgba(248,113,113,.03)',overflow:'hidden'}}>
        {[['Transférer l\'organisation','Donner la propriété à un autre administrateur.','Transférer'],['Exporter toutes les données','Projets, variables, logs et configuration au format JSON.','Exporter'],['Supprimer l\'organisation','Supprime définitivement les projets, bases et sauvegardes.','Supprimer']].map(([t2,d,b],i)=><div key={t2} style={{display:'flex',alignItems:'center',gap:16,padding:'18px 20px',borderTop:i?'1px solid rgba(248,113,113,.15)':'none'}}><span style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t2}</span><span style={lbl}>{d}</span></span><button onClick={()=>i===2?setDelOrg({pwd:'',word:'',show:false,busy:false}):flash(b+' : confirmation envoyée par e-mail')} style={{height:34,padding:'0 14px',borderRadius:10,cursor:'pointer',font:'600 12px/1 var(--font-sans)',border:'1px solid rgba(248,113,113,.4)',background:i===2?'#dc2626':'transparent',color:i===2?'#fff':'#f87171'}}>{b}</button></div>)}
      </div>}
      {delOrg&&(()=>{const {Modal}=window.CipheraDesignSystem;const ok=delOrg.pwd.length>=6&&delOrg.word==='SUPPRIMER';const fld={height:42,padding:'0 12px',borderRadius:10,border:'1px solid var(--border-default)',background:'var(--surface-raised)',color:'var(--text-strong)',font:'400 14px/1 var(--font-sans)',outline:'none',width:'100%',boxSizing:'border-box'};
        return <Modal title={'Supprimer '+s.name+' ?'} description="Cette action est définitive et ne peut pas être annulée." onClose={()=>setDelOrg(null)} width={480} footer={<><Button onClick={()=>setDelOrg(null)}>Annuler</Button><button disabled={!ok||delOrg.busy} onClick={()=>{setDelOrg(d=>({...d,busy:true}));setTimeout(()=>{setDelOrg(null);flash('Suppression de l\'organisation planifiée')},1200)}} style={{display:'flex',alignItems:'center',gap:8,height:36,padding:'0 14px',borderRadius:10,border:0,cursor:ok?'pointer':'default',background:ok?'#dc2626':'rgba(220,38,38,.35)',color:'#fff',font:'600 13px/1 var(--font-sans)'}}><Icon name={delOrg.busy?'loader':'trash-2'} size={14} color="#fff" style={delOrg.busy?{animation:'spin 1s linear infinite'}:undefined}/>Supprimer définitivement</button></>}>
          <div style={{display:'flex',flexDirection:'column',gap:16}}>
            <div style={{display:'flex',flexDirection:'column',gap:8,padding:'12px 14px',borderRadius:12,border:'1px solid rgba(248,113,113,.25)',background:'rgba(248,113,113,.05)'}}>{['Tous les projets et leurs déploiements','Les bases de données et sauvegardes','Les variables, secrets et logs','Les accès de tous les membres'].map(x=><span key={x} style={{display:'flex',alignItems:'center',gap:8,font:'400 13px/1.3 var(--font-sans)',color:'var(--text-body)'}}><Icon name="x" size={13} color="#f87171"/>{x}</span>)}</div>
            <label style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Mot de passe du compte</span><span style={{position:'relative',display:'flex',alignItems:'center'}}><input autoFocus type={delOrg.show?'text':'password'} value={delOrg.pwd} onChange={e=>setDelOrg(d=>({...d,pwd:e.target.value}))} style={{...fld,paddingRight:40}}/><button type="button" onClick={()=>setDelOrg(d=>({...d,show:!d.show}))} style={{position:'absolute',right:10,padding:0,border:0,background:'transparent',cursor:'pointer',display:'flex'}}><Icon name={delOrg.show?'eye-off':'eye'} size={16} color="var(--text-muted)"/></button></span></label>
            <label style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 13px/1.4 var(--font-sans)',color:'var(--text-strong)'}}>Tapez <span style={{fontFamily:'var(--font-mono)',padding:'1px 6px',borderRadius:5,background:'var(--surface-selected)',color:'#f87171'}}>SUPPRIMER</span> pour confirmer</span><input value={delOrg.word} onChange={e=>setDelOrg(d=>({...d,word:e.target.value}))} placeholder="SUPPRIMER" spellCheck="false" style={{...fld,fontFamily:'var(--font-mono)',letterSpacing:'.04em',borderColor:delOrg.word&&delOrg.word!=='SUPPRIMER'?'rgba(248,113,113,.5)':'var(--border-default)'}}/></label>
          </div>
        </Modal>})()}
    </div>
    </div>
    <Toast t={t}/>
  </div>;
}

/* ---------- Projets ---------- */
const FW={Railpack:'package',Dockerfile:'file-code'};
const TIC={Application:'globe',Worker:'cog','Base de données':'database','File de tâches':'list-ordered',Cache:'zap'};
function ProjCard({p,open,GH}){
  const [h,setH]=React.useState(false);const c=ENVC[p.env]||'237,237,237';const sv=((window.CIPHERA.services||{})[p.id]||[]).map(s=>s.n);
  const repo=p.repo.replace('github.com/','').replace('gitlab.com/','');
  const stc=p.status[0]==='error'?'#f87171':p.status[0]==='pending'?'#fbbf24':'#86efac';
  return <button onClick={()=>open(p.id)} onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{...box,display:'flex',flexDirection:'column',gap:16,padding:'18px 18px 16px',cursor:'pointer',textAlign:'left',background:h?'#101013':'#0c0c0f',borderColor:h?'rgba(255,255,255,.14)':'rgba(255,255,255,.08)',transition:'all .15s'}}>
    <div style={{display:'flex',alignItems:'center',gap:12}}>
      <span style={{width:36,height:36,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#17171b',border:'1px solid rgba(255,255,255,.08)'}}><Icon name={FW[p.build]||'layers'} size={16} color="var(--text-strong)"/></span>
      <span style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}><span style={{font:'600 14px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{p.name}</span><span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{repo}</span></span>
      <Icon name="arrow-up-right" size={14} color={h?'var(--text-strong)':'var(--text-faint)'}/>
    </div>
    <div style={{display:'flex',alignItems:'center',gap:8,font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}>
      <span style={{width:7,height:7,borderRadius:99,background:stc,flex:'none'}}></span>{p.status[1]}
      <span style={{flex:1}}></span>
      <span style={{display:'flex',alignItems:'center',gap:6,color:'var(--text-muted)'}}><span style={{width:6,height:6,borderRadius:2,background:'rgb('+c+')',opacity:.8}}></span>{p.env}</span>
    </div>
    <div style={{display:'flex',alignItems:'center',gap:6,paddingTop:12,borderTop:line,...lbl,fontSize:11,whiteSpace:'nowrap',overflow:'hidden'}}>
      <span style={{overflow:'hidden',textOverflow:'ellipsis'}}>{sv.length?sv.join(', '):p.services+' services'}</span><span style={{color:'var(--text-faint)'}}>·</span><span>{p.branch}</span><span style={{flex:1}}></span><span>{p.updated}</span>
    </div>
  </button>;
}
function ProjectsListV2({open,onImport}){
  const P=window.CIPHERA.projects;const SV=window.CIPHERA.services||{};const {Tabs:DTabs,Button,IconButton}=window.CipheraDesignSystem;
  const [tab,setTab]=React.useState('all');const [exp,setExp]=React.useState(null);const [hov,setHov]=React.useState(null);const [sort,setSort]=React.useState(['name',1]);const [t,flash]=useToast();
  const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};const hair='1px solid var(--border-subtle)';
  const F={all:()=>true,Production:p=>p.env==='Production',Preview:p=>p.env==='Preview',Staging:p=>p.env==='Staging',err:p=>p.status[0]==='error'};
  const rows=[...P.filter(F[tab])].sort((x,y)=>{const k=sort[0];const vx=k==='status'?x.status[0]:x[k],vy=k==='status'?y.status[0]:y[k];return (vx>vy?1:-1)*sort[1]});
  const H=(k,t2)=><button onClick={()=>setSort(s=>[k,s[0]===k?-s[1]:1])} style={{display:'flex',alignItems:'center',gap:6,padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 12px/1 var(--font-sans)',color:sort[0]===k?'var(--text-strong)':'var(--text-muted)'}}>{t2}<Icon name="arrow-down-up" size={12}/></button>;
  const COLS='28px minmax(0,1.6fr) minmax(0,1.1fr) minmax(0,1fr) 120px 136px';
  const Tile=({ic})=><span style={{position:'relative',width:56,height:56,flex:'none'}}><span style={{position:'absolute',left:6,right:6,bottom:-8,height:10,borderRadius:'50%',background:'rgba(0,0,0,.55)',filter:'blur(5px)'}}></span><span style={{position:'absolute',inset:0,borderRadius:15,transform:'rotate(-10deg)',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18),inset 0 -2px 0 rgba(0,0,0,.35)',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name={ic} size={23} color="#f2f2f2"/></span></span>;
  const ST={ok:['circle-check','var(--green-500)'],pending:['circle-alert','#e5b454'],error:['circle-x','#f87171']};
  const [menu,setMenu]=React.useState(null);
  React.useEffect(()=>{if(menu==null)return;const f=()=>setMenu(null);setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[menu]);
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:14}}>
    <div style={{...card,borderRadius:18,position:'relative',overflow:'hidden',display:'flex',alignItems:'center',gap:24,padding:'26px 28px'}}>
      <span style={{position:'absolute',inset:0,background:'radial-gradient(50% 140% at 100% 0%,rgba(139,124,246,.16),transparent 70%)',pointerEvents:'none'}}></span>
      <span style={{position:'relative',display:'flex',flexDirection:'column',gap:10,flex:1}}><span style={{font:'600 18px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Déployez un dépôt en quelques secondes</span><span style={{font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)',maxWidth:520}}>Importez depuis GitHub : Ciphera détecte le framework, construit l'image et la déploie sur vos serveurs.</span></span>
      <div style={{position:'relative',display:'flex',alignItems:'center',gap:12}}>
        <span style={{display:'flex'}}>{['../../web/src/assets/images/connectors/github-mark.png','../../web/src/assets/images/connectors/supabase-mark.png','../../web/src/assets/images/cloudflare/cloudflare-mark.png'].map((src,i)=><span key={i} style={{marginLeft:i?-8:0,width:34,height:34,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:'#16161a',border:'1px solid rgba(255,255,255,.12)',boxShadow:'0 0 0 3px #0c0c0f'}}><img src={src} alt="" style={{width:17,height:17,objectFit:'contain'}}/></span>)}</span>
        <Button variant="inverse" icon="plus" onClick={()=>onImport&&onImport()}>Importer un dépôt</Button>
      </div>
    </div>
    <div style={{paddingLeft:12,marginTop:6}}><DTabs tabs={[{value:'all',label:'Tous',count:P.length},{value:'Production',label:'Production',count:P.filter(F.Production).length},{value:'Preview',label:'Preview',count:P.filter(F.Preview).length},{value:'Staging',label:'Staging',count:P.filter(F.Staging).length},{value:'err',label:'En erreur',count:P.filter(F.err).length}]} value={tab} onChange={setTab}/></div>
    <div style={{...card,overflow:'visible'}}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:16,padding:'14px 16px'}}><span></span>{H('name','Détails')}{H('repo','Domaine')}{H('status','Statut')}{H('updated','Déployé')}<span></span></div>
      {rows.map(x=>{const o=exp===x.id,h=hov===x.id;const st=ST[x.status[0]]||ST.ok;const sv=(SV[x.id]||[]);const dom=x.name+'.ciphera.app';return <div key={x.id} style={{borderTop:hair}} onMouseEnter={()=>setHov(x.id)} onMouseLeave={()=>setHov(null)}>
        <div onClick={()=>setExp(o?null:x.id)} style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',padding:'14px 16px',cursor:'pointer',background:h&&!o?'var(--surface-raised)':'transparent'}}>
          <Icon name="chevron-down" size={15} color="var(--text-muted)" style={{transform:o?'rotate(180deg)':'none',transition:'transform var(--dur-fast)'}}/>
          <div style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}>
            <span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={FW[x.build]||'layers'} size={18} color="var(--text-strong)"/></span>
            <div style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}><span style={{flex:'none',whiteSpace:'nowrap'}}>{mono(x.name,'var(--text-strong)',13)}</span><span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{x.env} · {x.build} · {x.branch}</span></div>
          </div>
          <span style={{display:'flex',alignItems:'center',gap:6,minWidth:0}} onClick={e=>e.stopPropagation()}><span style={{overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{mono(dom,'var(--text-body)')}</span><IconButton size={26} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText('https://'+dom);flash('URL copiée')}}/></span>
          <span style={{display:'flex',alignItems:'center',gap:8}}><Icon name={st[0]} size={16} color={st[1]}/><span style={{font:'400 13px/1.2 var(--font-sans)',color:'var(--text-body)'}}>{x.status[1]}</span></span>
          <span><span style={{display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:x.status[0]==='error'?'rgba(229,180,84,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:x.status[0]==='error'?'#e5b454':'var(--text-body)'}}>{x.updated}</span></span>
          <div style={{display:'flex',gap:6,justifyContent:'flex-end',position:'relative'}} onClick={e=>e.stopPropagation()}>
            <Button size="sm" onClick={()=>open(x.id)}>Gérer</Button>
            <IconButton size={28} icon="ellipsis-vertical" label="Actions" onClick={()=>setMenu(m=>m===x.id?null:x.id)}/>
            {menu===x.id&&<div style={{position:'absolute',top:34,right:0,zIndex:30,width:210,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>{[['rotate-cw','Redéployer'],['scroll-text','Voir les logs'],['arrow-up-right','Ouvrir le site'],['git-branch','Voir le dépôt']].map(([ic,tt])=><button key={tt} onClick={()=>{setMenu(null);flash(tt+' · '+x.name)}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={ic} size={14}/>{tt}</button>)}</div>}
          </div>
        </div>
        {o&&<div style={{padding:'0 16px 16px'}}>{x.status[0]==='error'&&<div style={{display:'flex',alignItems:'center',gap:12,padding:'14px 18px',borderRadius:12,background:'rgba(239,68,68,.05)',border:'1px solid rgba(239,68,68,.18)',marginBottom:10}}><Icon name="circle-x" size={15} color="#f87171"/><span style={{flex:1,font:'400 12px/1.4 var(--font-sans)',color:'var(--text-body)'}}>Le dernier build a échoué. La version précédente reste en ligne.</span><Button size="sm" onClick={()=>flash('Redéploiement de '+x.name)}>Relancer</Button></div>}
          <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:24,padding:'16px 22px 18px',borderRadius:12,background:'var(--surface-raised)'}}>{[['Dépôt',x.repo.replace(/^(github|gitlab)\.com\//,'')],['Serveur',x.server],['Services',sv.length?sv.map(s=>s.n).join(', '):x.services],['Branche',x.branch]].map(([k,v])=><div key={k} style={{display:'flex',flexDirection:'column',gap:10,minWidth:0}}><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{k}</span><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{v}</span></div>)}</div>
        </div>}
      </div>})}
    </div>
    <Toast t={t}/>
  </div>;
}
function VSpark({d,w=112,h=44}){const m=Math.max(...d),n=Math.min(...d);const pts=d.map((v,i)=>[i/(d.length-1)*w,h-4-(v-n)/((m-n)||1)*(h-10)]);const path=pts.map((q,i)=>(i?'L':'M')+q[0].toFixed(1)+','+q[1].toFixed(1)).join(' ');const e=pts[pts.length-1];return <svg width={w} height={h} style={{flex:'none',overflow:'visible'}}><path d={path+' L'+w+','+h+' L0,'+h+' Z'} fill="rgba(139,124,246,.18)"/><path d={path} fill="none" stroke="#8b7cf6" strokeWidth="1.5"/><circle cx={e[0]} cy={e[1]} r="3.5" fill="#a78bfa"/></svg>}
function ExportBtn({S,cpu,ram,flash}){
  const [open,setOpen]=React.useState(false);const ref=React.useRef();const pop=React.useRef();const [pos,setPos]=React.useState({top:0,right:0});
  React.useEffect(()=>{if(!open)return;const r=ref.current.getBoundingClientRect();setPos({top:r.bottom+8,right:window.innerWidth-r.right});const f=e=>{if(!ref.current.contains(e.target)&&!(pop.current&&pop.current.contains(e.target)))setOpen(false)};document.addEventListener('mousedown',f);return()=>document.removeEventListener('mousedown',f)},[open]);
  const date=new Date().toISOString().slice(0,10);
  const rows=S.map(s=>({serveur:s.name,region:s.region,statut:s.status[1],cpu:s.status[0]==='error'?'':s.cpu,memoire:s.ram,disque:s.disk,conteneurs:s.containers,ip:s.ip,os:s.os,agent:s.agent}));
  const dl=(name,type,content)=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([content],{type}));a.download=name;document.body.appendChild(a);a.click();a.remove()};
  const FMT={csv:['CSV','table',()=>{const k=Object.keys(rows[0]);dl('ciphera-monitoring-'+date+'.csv','text/csv','\ufeff'+k.join(';')+'\n'+rows.map(r=>k.map(x=>'"'+String(r[x]).replace(/"/g,'""')+'"').join(';')).join('\n'))}],
    json:['JSON','braces',()=>dl('ciphera-monitoring-'+date+'.json','application/json',JSON.stringify({genere:new Date().toISOString(),cpu_moyen:cpu,memoire_moyenne:ram,serveurs:rows},null,2))],
    txt:['Rapport texte','file-text',()=>dl('ciphera-monitoring-'+date+'.txt','text/plain','Rapport Ciphera · '+date+'\n\nCPU moyen : '+cpu+' %\nMémoire moyenne : '+ram+' %\n\n'+rows.map(r=>r.serveur+'  ['+r.statut+']\n  CPU '+(r.cpu===''?'—':r.cpu+' %')+' · RAM '+r.memoire+' · Disque '+r.disque+' · '+r.conteneurs+' conteneurs').join('\n\n')+'\n')]};
  const run=k=>{FMT[k][2]();setOpen(false);flash('Rapport '+FMT[k][0]+' téléchargé')};
  return <div ref={ref} style={{display:'flex',borderRadius:10,overflow:'hidden'}}>
    <button onClick={()=>run('csv')} style={{height:38,padding:'0 16px',border:0,cursor:'pointer',background:'#8b7cf6',color:'#fff',font:'600 13px/1 var(--font-sans)'}} onMouseEnter={e=>e.currentTarget.style.background='#7c6cf0'} onMouseLeave={e=>e.currentTarget.style.background='#8b7cf6'}>Exporter le rapport</button>
    <button onClick={()=>setOpen(o=>!o)} style={{width:38,height:38,border:0,borderLeft:'1px solid rgba(0,0,0,.2)',cursor:'pointer',background:open?'#7c6cf0':'#8b7cf6',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="chevron-down" size={15} color="#fff" style={{transform:open?'rotate(180deg)':'none',transition:'transform .15s'}}/></button>
    {open&&ReactDOM.createPortal(<div ref={pop} style={{position:'fixed',top:pos.top,right:pos.right,zIndex:1000,width:240,padding:6,borderRadius:'var(--radius-lg)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)'}}>
      <span style={{display:'block',padding:'8px 10px 6px',font:'600 10px/1 var(--font-sans)',letterSpacing:'.08em',textTransform:'uppercase',color:'var(--text-subtle)'}}>Format</span>
      {Object.entries(FMT).map(([k,[n,ic]])=><button key={k} onClick={()=>run(k)} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'9px 10px',border:0,borderRadius:8,background:'transparent',cursor:'pointer',textAlign:'left',font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name={ic} size={14}/>{n}<span style={{flex:1}}></span><span style={{font:'400 11px/1 var(--font-mono)',color:'var(--text-subtle)'}}>.{k}</span></button>)}
    </div>,document.body)}
  </div>;
}
function MonitoringV2({go}){
  const D=window.CIPHERA;const S=D.servers;const {Badge,Button}=window.CipheraDesignSystem;
  const [live,setLive]=React.useState(true);const [tick,setTick]=React.useState(0);const [t,flash]=useToast();const [det,setDet]=React.useState(null);const [per,setPer]=React.useState('24 h');
  React.useEffect(()=>{if(!live)return;const id=setInterval(()=>setTick(x=>x+1),2500);return()=>clearInterval(id)},[live]);
  const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};const hair='1px solid var(--border-subtle)';
  const num=s=>parseFloat(String(s).replace(',','.'))||0;const on=S.filter(s=>s.status[0]!=='error');
  const j=v=>Math.max(0,Math.min(100,v+((tick*7+v)%5)-2));
  const cpu=Math.round(on.reduce((a,s)=>a+j(s.cpu),0)/on.length),ram=Math.round(on.reduce((a,s)=>a+num(s.ram)/num(s.ram.split('/')[1])*100,0)/on.length);
  const du=S.reduce((a,s)=>a+num(s.disk),0),dt=S.reduce((a,s)=>a+num(s.disk.split('/')[1]),0);
  const wave=(base,amp,seed,spiky)=>Array.from({length:40},(_,i)=>spiky?(i%6===((seed+i)%3)?base+amp:base*.2+((i*7)%3)):base+Math.sin((i+seed)/3)*amp+((i*13+seed)%7)-3);
  const ALERTS=[['circle-x','#f87171','Agent injoignable','srv-iad-01','il y a 3 h'],['circle-alert','#e5b454','CPU > 85 % pendant 10 min','srv-par-02','il y a 6 h']];
  const Tile=({title,val,unit,sub,spark,bar,k})=><button onClick={()=>setDet({title,val,unit,k})} onMouseEnter={e=>{e.currentTarget.style.background='var(--surface-raised)';e.currentTarget.style.borderColor='var(--border-strong)'}} onMouseLeave={e=>{e.currentTarget.style.background='transparent';e.currentTarget.style.borderColor='var(--border-default)'}} style={{display:'flex',flexDirection:'column',gap:14,padding:'18px 20px',borderRadius:14,border:'1px solid var(--border-default)',background:'transparent',cursor:'pointer',textAlign:'left',transition:'background .15s, border-color .15s'}}>
    <span style={{display:'flex',alignItems:'center'}}><span style={{flex:1,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><Icon name="chevron-right" size={14} color="var(--text-subtle)"/></span>
    <span style={{display:'flex',alignItems:'flex-end',gap:12}}><span style={{display:'flex',flexDirection:'column',gap:9,flex:1}}><span style={{display:'flex',alignItems:'baseline',gap:5}}><span style={{font:'600 24px/1 var(--font-sans)',color:'var(--text-strong)'}}>{val}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{unit}</span></span><span style={{...lbl,fontSize:11}}>{sub}</span></span>
      {spark?<VSpark d={spark}/>:<span style={{width:112,height:8,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden',marginBottom:6}}><span style={{display:'block',height:'100%',width:bar+'%',borderRadius:99,background:'#8b7cf6'}}></span></span>}
    </span>
  </button>;
  const Link=({ic,k,v,to})=><button onClick={to} onMouseEnter={e=>{e.currentTarget.style.background='var(--surface-raised)';e.currentTarget.style.borderColor='var(--border-strong)'}} onMouseLeave={e=>{e.currentTarget.style.background='var(--surface-app)';e.currentTarget.style.borderColor='var(--border-default)'}} style={{...card,display:'flex',alignItems:'center',gap:12,padding:'16px 18px',cursor:'pointer',textAlign:'left',transition:'background .15s, border-color .15s'}}><span style={{display:'flex',flexDirection:'column',gap:9,flex:1}}><span style={{display:'flex',alignItems:'center',gap:7,font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name={ic} size={13}/>{k}</span><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span></span><Icon name="chevron-right" size={15} color="var(--text-subtle)"/></button>;
  const bar=(k,v,txt)=><div style={{display:'flex',flexDirection:'column',gap:10,minWidth:0}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{k}</span><span style={{display:'flex',alignItems:'center',gap:12}}><span style={{flex:1,height:5,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:v+'%',borderRadius:99,background:v>=85?'#f87171':'#ededed'}}></span></span><span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)',width:38,textAlign:'right'}}>{txt}</span></span></div>;
  return <div style={{padding:'20px 32px 48px 34px',display:'flex',flexDirection:'column',gap:14}}>
    <div style={{...card,borderRadius:20,padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',alignItems:'center',gap:16}}>
        <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name="activity" size={24} color="#f2f2f2"/></span>
        <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>Infrastructure</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:ALERTS.length?'rgba(229,180,84,.12)':'rgba(34,197,94,.12)',font:'500 12px/1 var(--font-sans)',color:ALERTS.length?'#e5b454':'var(--green-500)'}}>{ALERTS.length?ALERTS.length+' alertes actives':'Opérationnelle'}</span></span><span style={lbl}>{S.length} serveurs · Paris, Francfort, Virginie</span></span>
        <ExportBtn S={S} cpu={cpu} ram={ram} flash={flash}/>
        <Button iconRight="arrow-up-right" onClick={()=>flash('Grafana ouvert')}>Grafana</Button>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
        <span>Agents : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>{on.length} / {S.length}</span></span><span style={{width:1,height:14,background:'var(--border-strong)'}}></span>
        <span>Version : <span style={{fontFamily:'var(--font-mono)',fontWeight:600,color:'var(--text-strong)'}}>0.9.4</span></span><span style={{width:1,height:14,background:'var(--border-strong)'}}></span>
        <span>Dernière collecte : <span style={{color:'var(--text-strong)'}}>{live?'il y a 2 s':'en pause'}</span></span>
        <span style={{flex:1}}></span>
        <button onClick={()=>setLive(v=>!v)} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'#a78bfa'}}>{live?'Mettre en pause':'Reprendre le direct'}</button>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:12}}>
        <Tile k="cpu" title="Utilisation du CPU" val={cpu} unit="%" sub="Dernières 24 heures" spark={wave(cpu,10,tick%40)}/>
        <Tile k="ram" title="Utilisation de la mémoire" val={ram} unit="%" sub="Dernières 24 heures" spark={wave(ram,3,5)}/>
        <Tile k="disk" title="Utilisation du disque" val={du} unit={'/ '+dt+' Go'} sub="Utilisation actuelle" bar={Math.round(du/dt*100)}/>
        <Tile k="in" title="Trafic entrant" val="12,4" unit="Go" sub="Dernières 24 heures" spark={wave(4,8,tick%6,true)}/>
        <Tile k="out" title="Trafic sortant" val="4,1" unit="Go" sub="Dernières 24 heures" spark={wave(20,9,tick%40+3)}/>
        <Tile k="bw" title="Bande passante" val="3,2" unit="To / 32 To" sub="Ce mois-ci" bar={10}/>
      </div>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:12}}>
      <Link ic="bell" k="Alertes actives" v={ALERTS.length} to={()=>{const el=document.getElementById('mon-alerts');if(el){const sc=el.closest('main');if(sc)sc.scrollTo({top:el.offsetTop-20,behavior:'smooth'});el.style.borderColor='#8b7cf6';setTimeout(()=>el.style.borderColor='',1200)}}}/><Link ic="box" k="Conteneurs" v={S.reduce((a,s)=>a+s.containers,0)} to={()=>go&&go('Conteneurs')}/><Link ic="archive" k="Sauvegardes" v="12" to={()=>go&&go('Bases de données')}/><Link ic="shield-half" k="Règles WAF" v="142" to={()=>go&&go('Sécurité')}/>
    </div>
    <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.5fr) minmax(0,1fr)',gap:14}}>
      <div style={{...card,borderRadius:18,overflow:'hidden'}}>
        <div style={{padding:'18px 22px',font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>Serveurs</div>
        {S.map(s=>{const off=s.status[0]==='error';const c=off?0:j(s.cpu),r=off?0:Math.round(num(s.ram)/num(s.ram.split('/')[1])*100),d=Math.round(num(s.disk)/num(s.disk.split('/')[1])*100);return <div key={s.id} style={{display:'flex',flexDirection:'column',gap:14,padding:'16px 22px',borderTop:hair}}>
          <span style={{display:'flex',alignItems:'center',gap:12}}>{window.OsLogo?<window.OsLogo os={s.os} size={32} dim={off}/>:null}{mono(s.name,'var(--text-strong)',13)}{off&&<Badge>Hors ligne</Badge>}<span style={{flex:1}}></span><span style={{...lbl,fontSize:11}}>{s.region.split(' · ')[0]}</span></span>
          <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:22,padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)'}}>{bar('Processeur',c,off?'—':c+' %')}{bar('Mémoire',r,off?'—':r+' %')}{bar('Disque',d,d+' %')}</div>
        </div>})}
      </div>
      <div id="mon-alerts" style={{...card,borderRadius:18,overflow:'hidden',alignSelf:'start',transition:'border-color .4s'}}>
        <div style={{display:'flex',alignItems:'center',padding:'18px 22px'}}><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>Alertes</span><span style={{flex:1}}></span><Badge>{ALERTS.length} actives</Badge></div>
        {ALERTS.map(([ic,c,tt,s,w])=><div key={tt} style={{display:'flex',alignItems:'center',justifyContent:'space-between',gap:12,padding:'16px 22px',borderTop:hair}}><span style={{display:'flex',alignItems:'center',gap:10,minWidth:0}}><Icon name={ic} size={15} color={c}/><span style={{font:'400 13px/1.3 var(--font-sans)',color:'var(--text-body)'}}>{tt}</span></span><span style={{display:'flex',flexDirection:'column',gap:5,alignItems:'flex-end',flex:'none'}}>{mono(s,'var(--text-strong)',12)}<span style={{...lbl,fontSize:11}}>{w}</span></span></div>)}
        {[['Seuil CPU','85 %'],['Seuil mémoire','90 %'],['Notification','E-mail + Slack']].map(([k,v])=><div key={k} style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'14px 22px',borderTop:hair}}><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>{k}</span><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span><Icon name="pencil" size={13} color="#a78bfa"/></span></div>)}
      </div>
    </div>
    {det&&(()=>{const {Modal}=window.CipheraDesignSystem;const N={'24 h':24,'7 j':7,'30 j':30}[per];
      const base={cpu:cpu,ram:ram,disk:Math.round(du/dt*100),in:40,out:22,bw:10}[det.k];const spiky=det.k==='in';
      const ser=Array.from({length:N},(_,i)=>Math.max(1,spiky?(i%5===2?base*1.8:base*.5+((i*7)%9)):base+Math.sin(i/2.4)*base*.18+((i*13)%7)-3));
      const lab=Array.from({length:N},(_,i)=>per==='24 h'?String(i).padStart(2,'0')+':00':D.days[D.days.length-N+i]||'');
      const unit=['cpu','ram','disk'].includes(det.k)?' %':det.k==='bw'?' %':' Go';
      const per1=v=>(det.k==='in'||det.k==='out')?(v/10).toFixed(1).replace('.',',')+' Go':Math.round(v)+' %';
      return <Modal title={det.title} description={'Tous les serveurs · '+det.val+' '+det.unit+' actuellement'} onClose={()=>setDet(null)} width={860}>
        <div style={{display:'flex',flexDirection:'column',gap:18}}>
          <div style={{display:'flex',alignItems:'center',gap:10}}>
            <div style={{display:'flex',gap:2,padding:3,borderRadius:10,border:'1px solid var(--border-default)'}}>{['24 h','7 j','30 j'].map(x=><button key={x} onClick={()=>setPer(x)} style={{height:28,padding:'0 12px',borderRadius:7,border:0,cursor:'pointer',font:'500 12px/1 var(--font-sans)',background:per===x?'var(--surface-selected)':'transparent',color:per===x?'var(--text-strong)':'var(--text-muted)'}}>{x}</button>)}</div>
            <span style={{flex:1}}></span>
            {[['Moyenne',per1(ser.reduce((a,b)=>a+b,0)/N)],['Pic',per1(Math.max(...ser))],['Minimum',per1(Math.min(...ser))]].map(([k2,v])=><span key={k2} style={{display:'flex',flexDirection:'column',gap:6,alignItems:'flex-end',paddingLeft:18}}><span style={{...lbl,fontSize:11}}>{k2}</span><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span></span>)}
          </div>
          <div style={{padding:'16px 4px 0'}}><AreaChart key={det.k+per} series={ser} labels={lab} fmt={per1} col="#8b7cf6"/></div>
          <div style={{borderRadius:12,border:'1px solid var(--border-default)',overflow:'hidden'}}>
            {S.map((s,i)=>{const off=s.status[0]==='error';const v=off?0:det.k==='cpu'?j(s.cpu):det.k==='ram'?Math.round(num(s.ram)/num(s.ram.split('/')[1])*100):det.k==='disk'?Math.round(num(s.disk)/num(s.disk.split('/')[1])*100):Math.round(base*(.6+i*.25));return <div key={s.id} style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) minmax(0,1.4fr) 60px',gap:16,alignItems:'center',padding:'12px 16px',borderTop:i?'1px solid var(--border-subtle)':'none'}}>
              <span style={{display:'flex',alignItems:'center',gap:10}}>{window.OsLogo?<window.OsLogo os={s.os} size={28} dim={off}/>:null}{mono(s.name,'var(--text-strong)',12)}</span>
              <span style={{height:5,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:Math.min(100,v)+'%',borderRadius:99,background:v>=85?'#f87171':'#8b7cf6'}}></span></span>
              <span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)',textAlign:'right'}}>{off?'—':(det.k==='in'||det.k==='out')?(v/10).toFixed(1).replace('.',',')+' Go':v+' %'}</span>
            </div>})}
          </div>
        </div>
      </Modal>})()}
    <Toast t={t}/>
  </div>;
}
Object.assign(window,{MonitoringV2,OverviewV2,DeploymentsPage,SettingsV2,ProjectsListV2});
})();
