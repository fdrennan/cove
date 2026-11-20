"""__HOST__: a blank Streamlit app. It reruns on every save."""
import streamlit as st

from log import log

# Streamlit runs this file again on every visit and widget change; the session
# state keeps it to one line per browser session. run.sh logs the start.
if "logged" not in st.session_state:
    st.session_state.logged = True
    log.info("session opened")

st.set_page_config(page_title="__HOST__")
st.title("__HOST__")
st.write("A blank Streamlit app, running. Save a file and it shows here.")
