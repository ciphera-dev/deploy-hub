(()=>{
const {Icon}=window.CipheraDesignSystem;
const HELP=['ls  df -h  free -h  uptime  docker ps  systemctl status ciphera  clear  exit'];
function out(cmd,host){
  const c=cmd.trim();if(!c)return [];
  const M={
    'ls':['apps  backups  ciphera  docker-compose.yml  logs'],
    'pwd':['/root'],'whoami':['root'],'hostname':[host],
    'uptime':[' 14:32:07 up 41 days,  3:12,  1 user,  load average: 0.38, 0.42, 0.40'],
    'df -h':['Filesystem      Size  Used Avail Use% Mounted on','/dev/vda1       160G   42G  118G  27% /','tmpfs           3.9G     0  3.9G   0% /dev/shm'],
    'free -h':['               total        used        free      shared  buff/cache   available','Mem:           7.8Gi       3.1Gi       1.2Gi       112Mi       3.5Gi       4.3Gi','Swap:          2.0Gi       128Mi       1.9Gi'],
    'docker ps':['CONTAINER ID   IMAGE                 STATUS          PORTS                NAMES','a3f91c2e0b14   ciphera/web:4821      Up 2 hours      0.0.0.0:3000->3000   web-1','7be0d14c9a21   postgres:17           Up 41 days      5432/tcp             postgres-1','c41a9e0f3d82   redis:7               Up 41 days      6379/tcp             redis-1'],
    'systemctl status ciphera':['● ciphera-agent.service - Ciphera agent','     Loaded: loaded (/etc/systemd/system/ciphera-agent.service; enabled)','     Active: active (running) since Mon 2026-10-12 11:20:04 UTC; 41 days ago','   Main PID: 812 (ciphera-agent)'],
    'help':HELP,
  };
  if(M[c])return M[c];
  if(c.startsWith('echo '))return [c.slice(5)];
  if(c.startsWith('cd '))return [];
  return [c.split(' ')[0]+': command not found'];
}
function TerminalModal({server,ip,onClose}){
  const host=server||'srv-par-01';
  const [lines,setLines]=React.useState([]);const [v,setV]=React.useState('');const [hist,setHist]=React.useState([]);const [hi,setHi]=React.useState(-1);const [ready,setReady]=React.useState(false);const [max,setMax]=React.useState(false);
  const ref=React.useRef();const inp=React.useRef();
  React.useEffect(()=>{const boot=[['dim','Connexion à '+host+' ('+(ip||'51.15.204.12')+') via l\'agent Ciphera…'],['dim','Authentification par clé · session chiffrée'],['out','Welcome to Ubuntu 24.04.1 LTS (GNU/Linux 6.8.0-49-generic x86_64)'],['out',''],['out','  System load:  0.38               Processes:             142'],['out','  Usage of /:   27.1% of 160GB    Users logged in:       0'],['out','  Memory usage: 40%                IPv4 address for eth0: '+(ip||'51.15.204.12')],['out',''],['dim','Tapez « help » pour la liste des commandes de démonstration.']];let i=0;const t=setInterval(()=>{const line=boot[i++];if(line)setLines(l=>[...l,line]);if(i>=boot.length){clearInterval(t);setReady(true)}},90);return()=>clearInterval(t)},[]);
  React.useEffect(()=>{if(ref.current)ref.current.scrollTop=ref.current.scrollHeight},[lines]);
  React.useEffect(()=>{if(ready&&inp.current)inp.current.focus()},[ready]);
  React.useEffect(()=>{const f=e=>{if(e.key==='Escape')onClose()};window.addEventListener('keydown',f);return()=>window.removeEventListener('keydown',f)},[]);
  const run=()=>{const c=v;setV('');setHi(-1);if(c.trim())setHist(h=>[c,...h]);
    if(c.trim()==='clear'){setLines([]);return}
    if(c.trim()==='exit'){setLines(l=>[...l,['cmd',c],['dim','logout · Connexion fermée.']]);setTimeout(onClose,500);return}
    setLines(l=>[...l,['cmd',c],...out(c,host).map(x=>['out',x])]);};
  const prompt=<span><span style={{color:'#86efac'}}>root@{host}</span><span style={{color:'var(--text-subtle)'}}>:</span><span style={{color:'#7dd3fc'}}>~</span><span style={{color:'var(--text-subtle)'}}>$ </span></span>;
  return ReactDOM.createPortal(<div onClick={onClose} style={{position:'fixed',inset:0,zIndex:1000,background:'rgba(0,0,0,.6)',backdropFilter:'blur(2px)',display:'flex',alignItems:'center',justifyContent:'center',padding:max?0:32}}>
    <div onClick={e=>{e.stopPropagation();inp.current&&inp.current.focus()}} style={{width:max?'100%':'min(980px,100%)',height:max?'100%':'min(620px,100%)',display:'flex',flexDirection:'column',borderRadius:max?0:16,overflow:'hidden',border:'1px solid var(--border-default)',background:'#0a0a0c',boxShadow:'0 30px 80px -20px rgba(0,0,0,.9)'}}>
      <div style={{display:'flex',alignItems:'center',gap:12,height:44,padding:'0 10px 0 16px',borderBottom:'1px solid var(--border-subtle)',background:'#111114'}}>
        <Icon name="terminal" size={15} color="var(--text-muted)"/>
        <span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>root@{host}</span>
        <span style={{display:'flex',alignItems:'center',gap:6,height:22,padding:'0 8px',borderRadius:6,background:ready?'rgba(34,197,94,.12)':'var(--surface-selected)',font:'500 11px/1 var(--font-sans)',color:ready?'var(--green-500)':'var(--text-muted)'}}><span style={{width:6,height:6,borderRadius:99,background:ready?'var(--green-500)':'var(--text-muted)'}}></span>{ready?'Connecté':'Connexion…'}</span>
        <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-subtle)'}}>SSH · {ip||'51.15.204.12'}:22</span>
        <span style={{flex:1}}></span>
        {[['eraser','Effacer',()=>setLines([])],[max?'minimize-2':'maximize-2',max?'Réduire':'Plein écran',()=>setMax(m=>!m)],['x','Fermer',onClose]].map(([ic,t,f])=><button key={t} title={t} onClick={e=>{e.stopPropagation();f()}} style={{width:30,height:30,border:0,borderRadius:7,background:'transparent',cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center'}} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'}><Icon name={ic} size={14} color="var(--text-muted)"/></button>)}
      </div>
      <div ref={ref} className="log-scroll" style={{flex:1,overflowY:'auto',padding:'14px 16px',font:'400 13px/1.6 var(--font-mono)',color:'#d4d4d8',whiteSpace:'pre-wrap',wordBreak:'break-all',cursor:'text'}}>
        {lines.filter(Boolean).map((l,i)=>l[0]==='cmd'?<div key={i}>{prompt}<span style={{color:'#f4f4f5'}}>{l[1]}</span></div>:<div key={i} style={{color:l[0]==='dim'?'var(--text-subtle)':l[1].includes('not found')?'#f87171':'#d4d4d8',minHeight:'1.6em'}}>{l[1]}</div>)}
        {ready&&<div style={{display:'flex'}}>{prompt}<input ref={inp} value={v} onChange={e=>setV(e.target.value)} onKeyDown={e=>{if(e.key==='Enter')run();else if(e.key==='ArrowUp'){e.preventDefault();const n=Math.min(hist.length-1,hi+1);if(n>=0){setHi(n);setV(hist[n])}}else if(e.key==='ArrowDown'){e.preventDefault();const n=hi-1;setHi(n);setV(n>=0?hist[n]:'')}else if(e.key==='l'&&e.ctrlKey){e.preventDefault();setLines([])}}} spellCheck="false" autoComplete="off" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',padding:0,color:'#f4f4f5',font:'inherit',caretColor:'#a78bfa'}}/></div>}
      </div>
      <div style={{display:'flex',alignItems:'center',gap:16,height:30,padding:'0 16px',borderTop:'1px solid var(--border-subtle)',background:'#111114',font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)'}}><span>↑ ↓ historique</span><span>Ctrl + L effacer</span><span>Échap fermer</span><span style={{flex:1}}></span><span>Session enregistrée dans le journal d'audit</span></div>
    </div>
  </div>,document.body);
}
window.TerminalModal=TerminalModal;
})();
