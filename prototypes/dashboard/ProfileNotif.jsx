(()=>{
const {Icon,Switch}=window.CipheraDesignSystem;
const {card,hair,rgba,Tile,Title}=window.ProfileUI;
const G=[['Déploiements',[['dep','circle-check','#22c08a','Déploiement réussi','Quand une version passe en production'],['fail','circle-x','#ef4444','Échec de build ou de déploiement','Avec les dernières lignes de log']]],['Infrastructure',[['sec','shield-alert','#f59e0b','Alertes de sécurité','Attaques bloquées, IP bannies, connexions suspectes'],['dns','globe','#38bdf8','Certificats et DNS','Expiration, renouvellement, erreurs Cloudflare']]],['Organisation',[['team','users','#8b6cff','Équipe','Invitations, changements de rôle'],['week','calendar','#ec4899','Rapport hebdomadaire','Résumé chaque lundi matin']]]];
function Check({on,onClick,disabled}){return <button onClick={onClick} disabled={disabled} style={{width:22,height:22,borderRadius:7,padding:0,cursor:disabled?'default':'pointer',outline:'none',display:'flex',alignItems:'center',justifyContent:'center',border:on?'none':'1.5px solid var(--gray-500)',background:on?'#ededed':'transparent',opacity:disabled?.35:1,transition:'all .15s'}}>{on&&<Icon name="check" size={13} color="#0b0b0b"/>}</button>}
function ProfileNotif(){
  const [n,setN]=React.useState({dep:[1,1],fail:[1,1],sec:[1,1],dns:[0,1],team:[1,0],week:[1,0]});
  const [ch,setCh]=React.useState([true,true]);const [quiet,setQuiet]=React.useState(false);
  const CH=[['mail','#38bdf8','E-mail','alex@example.com'],['layout-dashboard','#8b6cff','Dashboard','Cloche en haut à droite']];
  const col=j=>Object.values(n).filter(v=>v[j]).length;
  return <>
    <div style={{display:'grid',gridTemplateColumns:'repeat(auto-fill,minmax(200px,1fr))',gap:12}}>
      {CH.map(([ic,c,t,d],j)=><div key={t} style={{...card,padding:18,display:'flex',flexDirection:'column',gap:14,opacity:ch[j]?1:.6,transition:'opacity .2s'}}>
        <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}><Tile ic={ic} c={c} s={40}/><Switch checked={ch[j]} onChange={v=>setCh(x=>x.map((y,k)=>k===j?v:y))}/></div>
        <span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></span>
        <span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-muted)'}}>{col(j)} / 6 événements</span>
      </div>)}
      <div style={{...card,padding:18,display:'flex',flexDirection:'column',gap:14}}>
        <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}><Tile ic="moon" c="#a78bfa" s={40}/><Switch checked={quiet} onChange={setQuiet}/></div>
        <span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Ne pas déranger</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>22:00 – 08:00, sauf échecs</span></span>
        <span style={{font:'400 11px/1 var(--font-sans)',color:quiet?'#a78bfa':'var(--text-muted)'}}>{quiet?'Actif chaque nuit':'Désactivé'}</span>
      </div>
    </div>
    <Title t="Événements" d="Cochez les canaux pour chaque type d'événement."/>
    <div style={card}>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) 92px 92px',alignItems:'center',padding:'12px 18px',borderBottom:hair,background:'var(--surface-raised)'}}><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Événement</span>{CH.map(([ic,c,t])=><span key={t} style={{display:'flex',alignItems:'center',justifyContent:'center',gap:6,font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name={ic} size={12} color={c}/>{t}</span>)}</div>
      {G.map(([g,rows],gi)=><React.Fragment key={g}>
        <div style={{padding:'14px 18px 6px',borderTop:gi?hair:'none',font:'500 11px/1 var(--font-sans)',letterSpacing:'.06em',textTransform:'uppercase',color:'var(--text-faint)'}}>{g}</div>
        {rows.map(([k,ic,c,t,d])=><div key={k} onMouseEnter={e=>e.currentTarget.style.background='rgba(255,255,255,.02)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) 92px 92px',alignItems:'center',padding:'10px 18px'}}>
          <span style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}><Tile ic={ic} c={c} s={32}/><span style={{display:'flex',flexDirection:'column',gap:5,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{font:'400 12px/1.2 var(--font-sans)',color:'var(--text-subtle)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{d}</span></span></span>
          {[0,1].map(j=><span key={j} style={{display:'flex',justifyContent:'center'}}><Check on={!!n[k][j]&&ch[j]} disabled={!ch[j]} onClick={()=>setN(s=>({...s,[k]:s[k].map((x,m)=>m===j?(x?0:1):x)}))}/></span>)}
        </div>)}
      </React.Fragment>)}
    </div>
  </>;
}
window.ProfileNotif=ProfileNotif;
})();
