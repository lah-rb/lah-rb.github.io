module TailwindGroove exposing(main)

import Browser
import Html exposing (..)
import Html.Attributes exposing (..)
import Html.Events exposing(onClick)
import Array exposing(Array)

urlPrefix : String
urlPrefix = "http://elm-in-action.com/"

inFocus : String
inFocus = "border-4 border-teal-300 m-2"

outOfFocus : String
outOfFocus = "border border-black m-2"

mainHeading : String
mainHeading = "text-left text-teal-300 text-4xl font-bold bg-gray-800 my-4"

flexBox : String
flexBox = "flex content-start flex-wrap"



--viewThumbnail : String, {url : String} -> img
viewThumbnail selectedUrl thumb =
  if selectedUrl == thumb.url then
    img [class inFocus, src (urlPrefix ++ thumb.url)][]
  else
    img [class outOfFocus, src (urlPrefix ++ thumb.url), onClick {description = "clickedPhoto", data = thumb.url}][]

--update : {description : String, data : String}, model -> model
update msg model =
  if msg.description == "clickedPhoto" then
    {model | selectedUrl = msg.data}
  else
    model

--view : model -> Html
view model =
  div [class "content"][
    h1 [class mainHeading][text "Photo Groove"]
    , div [class flexBox][
        img [class outOfFocus, src (urlPrefix ++ "large/" ++ model.selectedUrl)][]
      , div [class flexBox, id "thumbnails"](List.map (viewThumbnail model.selectedUrl) model.photos)
      ]
    ]

initialModel : {photos : List {url : String}, selectedUrl : String}
initialModel =
  { photos = [ {url = "1.jpeg"}, {url = "2.jpeg"}, {url = "3.jpeg"}]
  , selectedUrl = "1.jpeg"
  }

photoArray : Array {url : String}
photoArray = Array.fromList initialModel.photos

main = Browser.sandbox {init = initialModel, view = view, update = update}
