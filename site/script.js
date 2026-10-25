// The picture of the notch at the top of a screen: it rests as a pill, opens on a page, and goes on to the next one by itself
// (until the visitor takes over with the tabs, or has asked for less motion).
(function () {
  var pages = {
    deadlines: 'The next deadline, a countdown, and everything coming up by course.',
    music: 'What is playing, with the lyrics in time.',
    shelf: 'Files you are carrying from one place to another.',
    controls: 'Volume, brightness, Wi-Fi and the rest, a click away.',
    projects: 'The tasks and files of a project in one small list.',
    clock: 'The time, the month and the weather.'
  }
  var order = Object.keys(pages)
  var stage = document.querySelector('.notch-stage')
  var images = stage.querySelectorAll('img.n')
  var rest = stage.querySelector('.rest')
  var tabs = document.querySelectorAll('.tabs [role=tab]')
  var caption = document.getElementById('caption')
  var calm = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches
  var current = null
  var timer = null
  var userChose = false

  function show (id) {
    current = id
    for (var i = 0; i < images.length; i++) {
      var on = images[i].getAttribute('data-page') === id
      images[i].classList.toggle('on', on)
    }
    rest.classList.remove('on')
    for (var t = 0; t < tabs.length; t++) tabs[t].setAttribute('aria-selected', String(tabs[t].getAttribute('data-go') === id))
    caption.textContent = pages[id]
  }

  function rested () {
    for (var i = 0; i < images.length; i++) images[i].classList.remove('on')
    rest.classList.add('on')
    caption.textContent = ''
  }

  function next () {
    var at = current ? order.indexOf(current) : -1
    show(order[(at + 1) % order.length])
  }

  function start () {
    if (calm || timer || userChose) return
    timer = window.setInterval(next, 4200)
  }
  function stop () {
    if (timer) { window.clearInterval(timer); timer = null }
  }

  for (var t = 0; t < tabs.length; t++) {
    tabs[t].addEventListener('click', function (event) {
      userChose = true
      stop()
      show(event.currentTarget.getAttribute('data-go'))
    })
  }
  document.addEventListener('visibilitychange', function () { if (document.hidden) stop(); else start() })

  // It rests for a moment first, as it does on a real screen, and then opens.
  rested()
  window.setTimeout(function () { show('deadlines'); start() }, calm ? 0 : 1300)
})()
