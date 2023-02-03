
function logout() {
    window.localStorage.removeItem('jwt_token');
    window.localStorage.removeItem('jwt_token_refresh');
    window.localStorage.removeItem('access_token');
    window.localStorage.removeItem('refresh_token');
    window.localStorage.removeItem('username');
    // compatibility with official Simple JWT
    window.localStorage.removeItem('access');
    window.localStorage.removeItem('refresh');
    window.location.href = "/api/__hidden_dev_dashboard";
}