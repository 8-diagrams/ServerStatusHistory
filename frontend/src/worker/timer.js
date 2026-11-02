let timer = null

onmessage = (e) => {
  if (e.data === 'start') {
    timer = setInterval(() => {
      postMessage('tick')
    }, 500)
  }
  if (e.data === 'stop') {
    clearInterval(timer)
    timer = null
  }
}
