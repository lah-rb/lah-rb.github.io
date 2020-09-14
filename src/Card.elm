import Browser
import HTML exposing (..)

cardModel =
  {  name = "Ribbon Coco Drak"
  ,  scarcity = "Unparalleled"
  ,  description = "Coco spirits take form of man in selfish death, beast in wrongful death, or a most powerful dragon in an sacrificing death. This spirit has clocked itself with ribbons for a attachment to the physical world."
  ,  play = "Requires taming."
  ,  body = "Sub Critical: ribbon, Critical: Energy Essence"
  }

view card =
  div [class "content"][
    h1 [][text cardModel.name]
  , ul [][
       li[][cardModel.scarcity]
    ,  li[][cardModel.description]
    ,  li[][cardModel.play]
    ,  li[][cardModel.body]
    ]
  ]

update model = model

main = Browser.sandbox {init = cardModel, view = view, update = update}
