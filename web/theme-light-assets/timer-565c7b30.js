(function(){"use strict";let t=null;onmessage=a=>{a.data==="start"&&(t=setInterval(()=>{postMessage("tick")},500)),a.data==="stop"&&(clearInterval(t),t=null)}})();
